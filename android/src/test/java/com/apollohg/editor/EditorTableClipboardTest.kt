package com.apollohg.editor

import android.app.Activity
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Point
import android.graphics.RectF
import android.net.Uri
import android.os.Looper
import android.view.DragEvent
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.widget.FrameLayout
import com.apollohg.editor.tables.TableCellDragShadow
import com.apollohg.editor.tables.TableCellDragState
import com.apollohg.editor.tables.activeTableCellPosition
import com.apollohg.editor.viewer.PreparedProseDrawingView
import com.apollohg.editor.viewer.TableCellDropTarget
import com.apollohg.editor.viewer.TableSelectionHandleRole
import java.time.Duration
import kotlin.math.ceil
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowWindowManagerGlobal
import org.robolectric.util.ReflectionHelpers

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], qualifiers = "w960dp-h640dp")
internal class EditorTableClipboardTest {
    private class RecordingBackend : EditorV2Backend by UniffiEditorV2Backend {
        val mutations = mutableListOf<String>()

        override fun applyCommand(
            editorId: String,
            requestJson: String
        ): EditorV2CallResult<String> {
            mutations += "$APPLY_COMMAND:" +
                JSONObject(requestJson).getJSONObject("command").getString("type")
            return UniffiEditorV2Backend.applyCommand(editorId, requestJson)
        }

        override fun applyInput(editorId: String, requestJson: String): EditorV2CallResult<String> {
            mutations += APPLY_INPUT
            return UniffiEditorV2Backend.applyInput(editorId, requestJson)
        }

        override fun setSelection(
            editorId: String,
            requestJson: String
        ): EditorV2CallResult<String> {
            mutations += SET_SELECTION
            return UniffiEditorV2Backend.setSelection(editorId, requestJson)
        }
    }

    private class Fixture(
        val token: Long,
        val view: RichTextEditorView,
        val adapter: EditorV2Adapter,
        val backend: RecordingBackend,
        val updates: MutableList<JSONObject>
    ) {
        val root: EditorEditText get() = view.editorEditText

        fun openings(tableIndex: Int = OUTER_TABLE): List<Int> {
            val table = adapter.tableRecordsForTesting.values.sortedBy {
                it.getInt("tablePos")
            }[tableIndex]
            val cells = table.getJSONArray("cells")
            return (0 until cells.length()).map { cells.getJSONObject(it).getInt("sourcePos") }
        }

        fun selectCells(anchor: Int, head: Int) {
            fun point(opening: Int) = JSONObject().put("kind", "document").put("offset", opening)
            select(
                JSONObject().put("type", CELL_SELECTION)
                    .put("anchorCell", point(anchor)).put("headCell", point(head))
            )
            assertTrue(
                "root did not adopt the cell selection",
                root.authoritativeCellSelectionActive
            )
            backend.mutations.clear()
            updates.clear()
        }

        fun relayout(target: RichTextEditorView = view) {
            target.measure(
                View.MeasureSpec.makeMeasureSpec(VIEW_WIDTH, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(VIEW_HEIGHT, View.MeasureSpec.EXACTLY)
            )
            target.layout(0, 0, VIEW_WIDTH, VIEW_HEIGHT)
            val drawing = target.editorTableSurface.drawingView
            drawing.measure(
                View.MeasureSpec.makeMeasureSpec(
                    target.editorEditText.width,
                    View.MeasureSpec.EXACTLY
                ),
                View.MeasureSpec.makeMeasureSpec(
                    target.editorEditText.height,
                    View.MeasureSpec.EXACTLY
                )
            )
            drawing.layout(0, 0, drawing.measuredWidth, drawing.measuredHeight)
            shadowOf(Looper.getMainLooper()).idle()
        }

        fun selectText(docPos: Int) {
            val point = JSONObject().put("kind", "scalar")
                .put("offset", requireNotNull(adapter.scalarPositionForDoc(docPos)))
            select(JSONObject().put("type", TEXT_SELECTION).put("anchor", point).put("head", point))
            backend.mutations.clear()
            updates.clear()
        }

        private fun select(selection: JSONObject) {
            val admitted = adapter.callWithEnvelope(JSONObject().put("selection", selection)) {
                UniffiEditorV2Backend.setSelection(adapter.editorId, it)
            }
            assertTrue(
                "engine rejected the selection: $admitted",
                admitted is EditorV2CallResult.Ok
            )
            assertTrue(root.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
        }

        fun nonOwnerView(): RichTextEditorView {
            val view = RichTextEditorView(RuntimeEnvironment.getApplication())
            view.editorId = token
            assertTrue(
                view.editorEditText.applyUpdateJSON(
                    requireNotNull(adapter.refreshFromRustState(null))
                )
            )
            view.measure(
                View.MeasureSpec.makeMeasureSpec(VIEW_WIDTH, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(VIEW_HEIGHT, View.MeasureSpec.EXACTLY)
            )
            view.layout(0, 0, VIEW_WIDTH, VIEW_HEIGHT)
            assertFalse(view.editorEditText.ownsNativeBinding(adapter))
            assertTrue(root.ownsNativeBinding(adapter))
            backend.mutations.clear()
            updates.clear()
            return view
        }

        fun engineSelection(): JSONObject {
            val result = UniffiEditorV2Backend.renderUpdate(adapter.editorId, null, null)
            assertTrue(result is EditorV2CallResult.Ok)
            return JSONObject((result as EditorV2CallResult.Ok).value).getJSONObject("selection")
        }

        fun rows(): JSONArray = JSONObject(requireNotNull(adapter.documentJson()))
            .getJSONArray("content").let { content ->
                (0 until content.length()).map(content::getJSONObject)
                    .first { it.getString("type") == TABLE_NODE }
            }.getJSONArray("content")

        fun cellTexts(): List<List<String>> {
            val rows = rows()
            return (0 until rows.length()).map { rowIndex ->
                val cells = rows.getJSONObject(rowIndex).getJSONArray("content")
                (0 until cells.length()).map { cellText(cells.getJSONObject(it)) }
            }
        }

        private fun cellText(node: JSONObject): String {
            node.optString("text").takeIf { node.optString("type") == TEXT_NODE }?.let { return it }
            val content = node.optJSONArray("content") ?: return ""
            return (0 until content.length()).joinToString("") {
                cellText(content.getJSONObject(it))
            }
        }
    }

    private fun withTable(
        document: String,
        schemaConfig: String = TABLE_CONFIG,
        attached: Boolean = false,
        block: (Fixture) -> Unit
    ) {
        val created = UniffiEditorV2Backend.create(schemaConfig, null) as EditorV2CallResult.Ok
        val backend = RecordingBackend()
        val adapter = requireNotNull(
            EditorV2Adapter.attach(backend, JSONObject(created.value).getString("editorId"), false)
        )
        val token = EditorV2Registry.register(adapter)
        try {
            val activity = if (attached) {
                Robolectric.buildActivity(
                    Activity::class.java
                ).setup()
            } else {
                null
            }
            val view = RichTextEditorView(activity?.get() ?: RuntimeEnvironment.getApplication())
            activity?.get()?.setContentView(
                FrameLayout(activity.get()).apply {
                    addView(view, FrameLayout.LayoutParams(VIEW_WIDTH, VIEW_HEIGHT))
                }
            )
            view.editorId = token
            assertTrue(
                view.editorEditText.applyUpdateJSON(
                    requireNotNull(adapter.setContentJson(document))
                )
            )
            view.measure(
                View.MeasureSpec.makeMeasureSpec(VIEW_WIDTH, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(VIEW_HEIGHT, View.MeasureSpec.EXACTLY)
            )
            view.layout(0, 0, VIEW_WIDTH, VIEW_HEIGHT)
            view.editorEditText.requestFocus()
            val updates = mutableListOf<JSONObject>()
            view.editorEditText.editorListener = object : EditorEditText.EditorListener {
                override fun onEditorUpdate(updateJSON: String) {
                    updates += JSONObject(updateJSON)
                }
                override fun onSelectionChanged(anchor: Int, head: Int) = Unit
            }
            assertFalse(
                "fixture must start without history",
                requireNotNull(adapter.historyCanUndo())
            )
            if (attached) shadowOf(Looper.getMainLooper()).idle()
            try {
                block(Fixture(token, view, adapter, backend, updates))
            } finally {
                activity?.pause()?.stop()?.destroy()
            }
        } finally {
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    private fun clipboard(): ClipboardManager = RuntimeEnvironment.getApplication()
        .getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager

    private fun shortcut(keyCode: Int) =
        KeyEvent(0L, 0L, KeyEvent.ACTION_DOWN, keyCode, 0, KeyEvent.META_CTRL_ON)

    private fun assertOneUndoableMutation(fixture: Fixture, command: String, before: String?) {
        assertEquals(
            "exactly one engine mutation and no selection rewrite",
            listOf("$APPLY_COMMAND:$command"),
            fixture.backend.mutations
        )
        assertEquals("exactly one published update", 1, fixture.updates.size)
        assertTrue(requireNotNull(fixture.adapter.historyCanUndo()))
        fixture.root.applyUpdateJSON(requireNotNull(fixture.adapter.undo()))
        assertEquals("one undo must restore the table", before, fixture.adapter.documentJson())
        assertFalse(
            "the action must be a single history entry",
            requireNotNull(fixture.adapter.historyCanUndo())
        )
    }

    @Test
    fun `copying a merged rectangle publishes exact tsv html and fragment without an update`() =
        withTable(MERGED_DOCUMENT) { fixture ->
            val openings = fixture.openings()
            fixture.selectCells(openings[MERGED_WIDE_CELL], openings[MERGED_SECOND_ROW_MIDDLE_CELL])
            val beforeDocument = fixture.adapter.documentJson()
            val beforeRevision = fixture.adapter.baseDocumentRevision
            clipboard().setPrimaryClip(ClipData.newPlainText(STALE_LABEL, STALE_TEXT))

            assertTrue(fixture.root.dispatchKeyEvent(shortcut(KeyEvent.KEYCODE_C)))

            val clip = requireNotNull(clipboard().primaryClip)
            assertEquals(MERGED_RECTANGLE_TSV, clip.getItemAt(0).text.toString())
            assertEquals(MERGED_RECTANGLE_HTML, clip.getItemAt(0).htmlText)
            assertTrue(clip.description.hasMimeType(ClipDescription.MIMETYPE_TEXT_HTML))
            assertTrue(clip.description.hasMimeType(ClipDescription.MIMETYPE_TEXT_PLAIN))
            assertTrue(clip.description.hasMimeType(EditorClipboard.MIME_TYPE_FRAGMENT))
            val fragment = JSONObject(
                requireNotNull(clip.description.extras?.getString(EditorClipboard.EXTRA_FRAGMENT))
            )
            assertEquals(MERGED_RECTANGLE_TSV, fragment.getString("text"))
            val copiedRows = fragment.getJSONObject("document").getJSONArray("content")
                .getJSONObject(0).getJSONArray("content")
            assertEquals(
                MERGED_COLSPAN,
                copiedRows.getJSONObject(0).getJSONArray("content").getJSONObject(0)
                    .getJSONObject("attrs").getInt("colspan")
            )
            assertEquals("copy must not mutate", emptyList<String>(), fixture.backend.mutations)
            assertEquals("copy must not publish an update", 0, fixture.updates.size)
            assertEquals(beforeDocument, fixture.adapter.documentJson())
            assertEquals(beforeRevision, fixture.adapter.baseDocumentRevision)
            assertFalse(requireNotNull(fixture.adapter.historyCanUndo()))
            assertEquals(CELL_SELECTION, fixture.engineSelection().getString("type"))
        }

    @Test
    fun `cutting cells publishes the copy and clears them in one undoable mutation`() =
        withTable(GRID_DOCUMENT) { fixture ->
            val openings = fixture.openings()
            fixture.selectCells(openings[FIRST_CELL], openings[SECOND_CELL])
            val before = fixture.adapter.documentJson()

            assertTrue(fixture.root.onTextContextMenuItem(android.R.id.cut))

            assertEquals(
                FIRST_ROW_TSV,
                requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString()
            )
            assertEquals(listOf(listOf("", ""), listOf("C", "D")), fixture.cellTexts())
            assertEquals(CELL_SELECTION, fixture.engineSelection().getString("type"))
            assertOneUndoableMutation(fixture, DELETE_BACKWARD_COMMAND, before)
        }

    @Test
    fun `pasting tsv into a cell selection fills the grid without replacing the selection`() =
        withTable(GRID_DOCUMENT) { fixture ->
            val openings = fixture.openings()
            fixture.selectCells(openings[FIRST_CELL], openings[LAST_CELL])
            val before = fixture.adapter.documentJson()
            clipboard().setPrimaryClip(ClipData.newPlainText(STALE_LABEL, PASTED_GRID_TSV))

            assertTrue(fixture.root.dispatchKeyEvent(shortcut(KeyEvent.KEYCODE_V)))

            assertEquals(listOf(listOf("w", "x"), listOf("y", "z")), fixture.cellTexts())
            val selection = fixture.engineSelection()
            assertEquals(CELL_SELECTION, selection.getString("type"))
            assertEquals(openings[FIRST_CELL], selection.getInt("anchorCell"))
            assertEquals(openings[LAST_CELL], selection.getInt("headCell"))
            assertOneUndoableMutation(fixture, PASTE_COMMAND, before)
        }

    @Test
    fun `rich paste prefers the html table and plain paste uses its text`() =
        withTable(GRID_DOCUMENT) { fixture ->
            val openings = fixture.openings()
            fixture.selectCells(openings[FIRST_CELL], openings[SECOND_CELL])
            val before = fixture.adapter.documentJson()
            clipboard().setPrimaryClip(
                ClipData(
                    ClipDescription(
                        STALE_LABEL,
                        arrayOf(
                            ClipDescription.MIMETYPE_TEXT_HTML,
                            ClipDescription.MIMETYPE_TEXT_PLAIN
                        )
                    ),
                    ClipData.Item(PLAIN_ALTERNATIVE_TSV, HTML_TABLE)
                )
            )

            assertTrue(fixture.root.onTextContextMenuItem(android.R.id.paste))
            assertEquals(listOf(listOf("h1", "h2"), listOf("C", "D")), fixture.cellTexts())
            assertOneUndoableMutation(fixture, PASTE_COMMAND, before)

            fixture.selectCells(openings[FIRST_CELL], openings[SECOND_CELL])
            assertTrue(fixture.root.onTextContextMenuItem(android.R.id.pasteAsPlainText))
            assertEquals(listOf(listOf("p1", "p2"), listOf("C", "D")), fixture.cellTexts())
            assertOneUndoableMutation(fixture, PASTE_COMMAND, before)
        }

    @Test
    fun `nested read only cells copy but never reach the planner for cut or paste`() =
        withTable(EditorTableSurfaceMountTest.nestedTableDocument) { fixture ->
            val nested = fixture.openings(NESTED_TABLE)
            fixture.selectCells(nested[FIRST_CELL], nested[FIRST_CELL])
            val before = fixture.adapter.documentJson()
            val notesBefore = fixture.adapter.debugNotes.size

            assertTrue(fixture.root.onTextContextMenuItem(android.R.id.copy))
            assertEquals(
                NESTED_CELL_TEXT,
                requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString()
            )
            clipboard().setPrimaryClip(ClipData.newPlainText(STALE_LABEL, PASTED_GRID_TSV))
            assertTrue(fixture.root.onTextContextMenuItem(android.R.id.cut))
            assertEquals(
                "a refused cut must keep the clipboard",
                PASTED_GRID_TSV,
                requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString()
            )
            assertTrue(fixture.root.onTextContextMenuItem(android.R.id.paste))

            assertEquals(emptyList<String>(), fixture.backend.mutations)
            assertEquals(
                "a refused edit must not emit an error",
                emptyList<String>(),
                fixture.adapter.debugNotes.drop(notesBefore).filter {
                    it.startsWith(EMITTED_ERROR_NOTE)
                }
            )
            assertEquals(0, fixture.updates.size)
            assertEquals(before, fixture.adapter.documentJson())
            assertFalse(requireNotNull(fixture.adapter.historyCanUndo()))
        }

    @Test
    fun `a view that does not own the table cannot cut or paste its cell selection`() =
        withTable(GRID_DOCUMENT) { fixture ->
            val openings = fixture.openings()
            fixture.selectCells(openings[FIRST_CELL], openings[LAST_CELL])
            val stale = fixture.nonOwnerView().editorEditText
            assertTrue(
                "the stale view adopted the cell selection",
                stale.authoritativeCellSelectionActive
            )
            val before = fixture.adapter.documentJson()
            clipboard().setPrimaryClip(ClipData.newPlainText(STALE_LABEL, PASTED_GRID_TSV))

            assertTrue(stale.onTextContextMenuItem(android.R.id.cut))
            assertTrue(stale.dispatchKeyEvent(shortcut(KeyEvent.KEYCODE_V)))

            assertEquals(emptyList<String>(), fixture.backend.mutations)
            assertEquals(
                PASTED_GRID_TSV,
                requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString()
            )
            assertEquals(before, fixture.adapter.documentJson())
            assertFalse(requireNotNull(fixture.adapter.historyCanUndo()))

            assertTrue(fixture.root.onTextContextMenuItem(android.R.id.paste))
            assertEquals(listOf("$APPLY_COMMAND:$PASTE_COMMAND"), fixture.backend.mutations)
        }

    @Test
    fun `clearing selected cells refuses a text selection without a mutation`() =
        withTable(GRID_DOCUMENT) { fixture ->
            fixture.selectText(fixture.openings()[FIRST_CELL] + CELL_TEXT_OFFSET)
            val before = fixture.adapter.documentJson()

            assertEquals(null, fixture.adapter.clearSelectedTableCells())

            assertEquals(emptyList<String>(), fixture.backend.mutations)
            assertEquals(before, fixture.adapter.documentJson())
        }

    @Test
    fun `atom cells copy and paste with their payload intact`() =
        withTable(ATOM_DOCUMENT, ATOM_TABLE_CONFIG) { fixture ->
            val openings = fixture.openings()
            fixture.selectCells(openings[FIRST_CELL], openings[SECOND_CELL])
            assertTrue(fixture.root.onTextContextMenuItem(android.R.id.copy))
            assertTrue(
                requireNotNull(clipboard().primaryClip).description.extras
                    ?.getString(
                        EditorClipboard.EXTRA_FRAGMENT
                    ).orEmpty().contains(ATOM_METADATA_KIND)
            )

            fixture.selectCells(openings[THIRD_CELL], openings[LAST_CELL])
            val before = fixture.adapter.documentJson()
            assertTrue(fixture.root.onTextContextMenuItem(android.R.id.paste))

            val rows = fixture.rows()
            assertEquals(
                rows.getJSONObject(0).getJSONArray("content").toString(),
                rows.getJSONObject(1).getJSONArray("content").toString()
            )
            assertOneUndoableMutation(fixture, PASTE_COMMAND, before)
        }

    private fun drawing(fixture: Fixture): PreparedProseDrawingView =
        fixture.view.editorTableSurface.drawingView

    private fun cellCenter(fixture: Fixture, cell: Int): Pair<Float, Float> {
        val drawing = drawing(fixture)
        val presented = drawing.presentedTableCells().single { it.sourceIndex == cell }
        return presented.bounds.centerX() + drawing.left to presented.bounds.centerY() + drawing.top
    }

    private fun dispatchFrameTouches(
        fixture: Fixture,
        points: List<Pair<Int, Pair<Float, Float>>>
    ) {
        points.forEachIndexed { index, (action, point) ->
            val event = MotionEvent.obtain(
                0,
                TOUCH_STEP_MS * index,
                action,
                point.first,
                point.second,
                0
            )
            try {
                fixture.view.editorContentFrame.dispatchTouchEvent(event)
            } finally {
                event.recycle()
            }
        }
    }

    private fun dragHead(fixture: Fixture, toCell: Int) {
        val drawing = drawing(fixture)
        val head = drawing.selectionHandles().single { it.role == TableSelectionHandleRole.HEAD }
        val target = cellCenter(fixture, toCell)
        dispatchFrameTouches(
            fixture,
            listOf(
                MotionEvent.ACTION_DOWN to (head.x + drawing.left to head.y + drawing.top),
                MotionEvent.ACTION_MOVE to target,
                MotionEvent.ACTION_UP to target
            )
        )
        assertEquals(
            "the drag must land on the target cell",
            fixture.openings()[toCell],
            fixture.engineSelection().getInt("headCell")
        )
    }

    private fun awaitDoubleTapTimeout() = shadowOf(
        Looper.getMainLooper()
    ).idleFor(Duration.ofMillis(ViewConfiguration.getDoubleTapTimeout().toLong()))

    private fun tapCell(fixture: Fixture, cell: Int) {
        val center = cellCenter(fixture, cell)
        dispatchFrameTouches(
            fixture,
            listOf(
                MotionEvent.ACTION_DOWN to center,
                MotionEvent.ACTION_UP to center
            )
        )
    }

    private fun tapCellAndSettle(fixture: Fixture, cell: Int) {
        tapCell(fixture, cell)
        awaitDoubleTapTimeout()
    }

    private fun selectCellsForMenu(fixture: Fixture, anchor: Int, head: Int) {
        val openings = fixture.openings()
        fixture.selectCells(openings[anchor], openings[head])
        fixture.relayout()
    }

    private fun showMenuByTappingSelection(fixture: Fixture, anchor: Int, head: Int) {
        selectCellsForMenu(fixture, anchor, head)
        tapCellAndSettle(fixture, anchor)
        assertTrue(
            "a tap inside the selection shows the cell menu",
            fixture.view.editorTableSurface.isCellEditMenuVisible
        )
        assertTrue(
            "the tap keeps the cell selection",
            fixture.root.authoritativeCellSelectionActive
        )
        fixture.backend.mutations.clear()
        fixture.updates.clear()
    }

    private fun menuItemIds(fixture: Fixture): List<Int> {
        val mode = requireNotNull(fixture.root.selectionActionMode) { "no action mode is showing" }
        assertTrue("the cell menu owns the action mode slot", mode.tag !== TextSelectionActionMode)
        return (0 until mode.menu.size()).map { mode.menu.getItem(it).itemId }
    }

    private fun clickMenuItem(fixture: Fixture, id: Int) {
        val mode = requireNotNull(fixture.root.selectionActionMode)
        assertTrue("menu item $id was not handled", mode.menu.performIdentifierAction(id, 0))
        assertFalse(
            "an item closes the menu",
            fixture.view.editorTableSurface.isCellEditMenuVisible
        )
    }

    private fun assertCellMenuReplacesTextMenu(clip: ClipData?, expected: List<Int>) =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            if (clip == null) clipboard().clearPrimaryClip() else clipboard().setPrimaryClip(clip)
            val text = requireNotNull(fixture.root.text).toString()
            val after = text.indexOf(AFTER_TEXT)
            fixture.relayout()
            fixture.root.setSelection(after, after + AFTER_TEXT.length)
            fixture.root.interaction.startSelectionActionMode()
            val textMenu = requireNotNull(fixture.root.selectionActionMode)
            assertTrue(textMenu.tag === TextSelectionActionMode)

            val openings = fixture.openings()
            fixture.selectCells(openings[FIRST_CELL], openings[SECOND_CELL])
            assertTrue(
                "the cell menu replaces the text menu",
                fixture.view.editorTableSurface.isCellEditMenuVisible
            )
            assertTrue(fixture.root.selectionActionMode !== textMenu)
            assertEquals(expected, menuItemIds(fixture))

            fixture.root.interaction.startSelectionActionMode()
            assertFalse(
                "a text menu closes the cell menu",
                fixture.view.editorTableSurface.isCellEditMenuVisible
            )
            assertTrue(
                requireNotNull(fixture.root.selectionActionMode).tag === TextSelectionActionMode
            )
        }

    @Test
    fun `handle drag end keeps the menu closed and a tap inside shows cut copy and paste`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            clipboard().setPrimaryClip(ClipData.newPlainText(STALE_LABEL, PASTED_GRID_TSV))
            selectCellsForMenu(fixture, FIRST_CELL, FIRST_CELL)
            dragHead(fixture, LAST_CELL)
            assertFalse(
                "a handle drag leaves the toolbar in charge",
                fixture.view.editorTableSurface.isCellEditMenuVisible
            )
            assertEquals(null, fixture.root.selectionActionMode)

            tapCell(fixture, FIRST_CELL)
            assertFalse(
                "the menu waits for a possible double tap",
                fixture.view.editorTableSurface.isCellEditMenuVisible
            )
            awaitDoubleTapTimeout()
            assertTrue(fixture.view.editorTableSurface.isCellEditMenuVisible)
            assertEquals(CELL_MENU_ITEMS, menuItemIds(fixture))
            val menu = requireNotNull(fixture.root.selectionActionMode).menu
            assertTrue((0 until menu.size()).all { !menu.getItem(it).title.isNullOrEmpty() })
        }

    @Test
    fun `copy menu item copies the cells without a mutation`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            showMenuByTappingSelection(fixture, FIRST_CELL, SECOND_CELL)
            val before = fixture.adapter.documentJson()
            clickMenuItem(fixture, android.R.id.copy)
            assertEquals(
                FIRST_ROW_TSV,
                requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString()
            )
            assertEquals("copy must not mutate", emptyList<String>(), fixture.backend.mutations)
            assertEquals(0, fixture.updates.size)
            assertEquals(before, fixture.adapter.documentJson())
        }

    @Test
    fun `cut menu item clears the cells in one undoable mutation`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            showMenuByTappingSelection(fixture, FIRST_CELL, SECOND_CELL)
            val before = fixture.adapter.documentJson()
            clickMenuItem(fixture, android.R.id.cut)
            assertEquals(
                FIRST_ROW_TSV,
                requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString()
            )
            assertEquals(listOf(listOf("", ""), listOf("C", "D")), fixture.cellTexts())
            assertOneUndoableMutation(fixture, DELETE_BACKWARD_COMMAND, before)
        }

    @Test
    fun `paste menu item fills the selection in one undoable mutation`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            clipboard().setPrimaryClip(ClipData.newPlainText(STALE_LABEL, PASTED_GRID_TSV))
            showMenuByTappingSelection(fixture, FIRST_CELL, LAST_CELL)
            val before = fixture.adapter.documentJson()
            clickMenuItem(fixture, android.R.id.paste)
            assertEquals(listOf(listOf("w", "x"), listOf("y", "z")), fixture.cellTexts())
            assertOneUndoableMutation(fixture, PASTE_COMMAND, before)
        }

    @Test
    fun `paste menu item is absent with an empty clipboard`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            clipboard().clearPrimaryClip()
            showMenuByTappingSelection(fixture, FIRST_CELL, SECOND_CELL)
            assertEquals(listOf(android.R.id.cut, android.R.id.copy), menuItemIds(fixture))
        }

    @Test
    fun `paste menu item is offered for a clip that only coerces to text`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            clipboard().setPrimaryClip(
                ClipData(
                    ClipDescription(STALE_LABEL, arrayOf(IMAGE_MIME_TYPE)),
                    ClipData.Item(Uri.parse(CONTENT_URI))
                )
            )
            showMenuByTappingSelection(fixture, FIRST_CELL, SECOND_CELL)
            assertEquals(CELL_MENU_ITEMS, menuItemIds(fixture))
        }

    @Test
    fun `read only nested cells show a copy only menu whose copy never mutates`() =
        withTable(EditorTableSurfaceMountTest.nestedTableDocument, attached = true) { fixture ->
            clipboard().setPrimaryClip(ClipData.newPlainText(STALE_LABEL, PASTED_GRID_TSV))
            val nested = fixture.openings(NESTED_TABLE).first()
            fixture.selectCells(nested, nested)
            fixture.relayout()
            val before = fixture.adapter.documentJson()
            val drawing = drawing(fixture)
            val selected = requireNotNull(
                drawing.selectedTableCellRects(drawing.selectedTableCellSourceIndices.keys.single())
            ).single()
            val center = selected.centerX() + drawing.left to selected.centerY() + drawing.top
            dispatchFrameTouches(
                fixture,
                listOf(
                    MotionEvent.ACTION_DOWN to center,
                    MotionEvent.ACTION_UP to center
                )
            )
            awaitDoubleTapTimeout()
            assertTrue(
                "a tap inside the read-only selection shows the menu",
                fixture.view.editorTableSurface.isCellEditMenuVisible
            )
            assertEquals(listOf(android.R.id.copy), menuItemIds(fixture))

            clickMenuItem(fixture, android.R.id.copy)

            assertEquals(
                NESTED_CELL_TEXT,
                requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString()
            )
            assertEquals("copy must not mutate", emptyList<String>(), fixture.backend.mutations)
            assertEquals(0, fixture.updates.size)
            assertEquals(before, fixture.adapter.documentJson())
        }

    @Test
    fun `a view that does not own the table shows a copy only menu`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            clipboard().setPrimaryClip(ClipData.newPlainText(STALE_LABEL, PASTED_GRID_TSV))
            val openings = fixture.openings()
            fixture.selectCells(openings[FIRST_CELL], openings[LAST_CELL])
            val stale = fixture.nonOwnerView()
            (fixture.view.parent as FrameLayout).addView(
                stale,
                FrameLayout.LayoutParams(VIEW_WIDTH, VIEW_HEIGHT)
            )
            fixture.relayout(stale)
            assertTrue(stale.editorEditText.requestFocus())
            assertTrue(stale.editorEditText.authoritativeCellSelectionActive)

            stale.editorTableSurface.presentCellEditMenu()

            assertTrue(stale.editorTableSurface.isCellEditMenuVisible)
            val mode = requireNotNull(stale.editorEditText.selectionActionMode)
            assertEquals(
                listOf(android.R.id.copy),
                (0 until mode.menu.size()).map {
                    mode.menu.getItem(it).itemId
                }
            )
            assertTrue(mode.menu.performIdentifierAction(android.R.id.copy, 0))
            assertEquals(
                FIRST_ROW_TSV + "\n" + "C\tD",
                requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString()
            )
            assertEquals(
                "a non-owner copy never mutates",
                emptyList<String>(),
                fixture.backend.mutations
            )
        }

    @Test
    fun `a text action mode is replaced by the full cell menu when the clipboard has text`() =
        assertCellMenuReplacesTextMenu(
            ClipData.newPlainText(STALE_LABEL, PASTED_GRID_TSV),
            CELL_MENU_ITEMS
        )

    @Test
    fun `empty clipboard cell menu replaces text actions without paste`() =
        assertCellMenuReplacesTextMenu(null, listOf(android.R.id.cut, android.R.id.copy))

    @Test
    fun `tap inside the selection toggles the menu and a tap outside edits that cell`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            selectCellsForMenu(fixture, FIRST_CELL, SECOND_CELL)
            tapCellAndSettle(fixture, FIRST_CELL)
            assertTrue(fixture.view.editorTableSurface.isCellEditMenuVisible)
            tapCellAndSettle(fixture, SECOND_CELL)
            assertFalse(fixture.view.editorTableSurface.isCellEditMenuVisible)
            tapCellAndSettle(fixture, FIRST_CELL)
            assertTrue(fixture.view.editorTableSurface.isCellEditMenuVisible)

            tapCellAndSettle(fixture, LAST_CELL)
            assertFalse(
                "a touch outside the selection closes the menu",
                fixture.view.editorTableSurface.isCellEditMenuVisible
            )
            assertTrue(
                "the outside tap edits that cell",
                fixture.view.activeTextInput !== fixture.root
            )
            assertEquals(
                emptyList<String>(),
                fixture.backend.mutations.filter {
                    it.startsWith(APPLY_COMMAND)
                }
            )
        }

    @Test
    fun `a touch outside the table closes the menu`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            showMenuByTappingSelection(fixture, FIRST_CELL, SECOND_CELL)
            val drawing = drawing(fixture)
            val below =
                drawing.selectedTableCellRects(
                    requireNotNull(drawing.selectedTableCellEndpoints).first
                )
                    .orEmpty().maxOf { it.bottom } + drawing.top + OUTSIDE_TOUCH_OFFSET
            dispatchFrameTouches(
                fixture,
                listOf(MotionEvent.ACTION_DOWN to (cellCenter(fixture, FIRST_CELL).first to below))
            )
            assertFalse(fixture.view.editorTableSurface.isCellEditMenuVisible)
        }

    @Test
    fun `double tap inside the selection edits the tapped cell without flashing the menu`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            selectCellsForMenu(fixture, FIRST_CELL, LAST_CELL)
            tapCell(fixture, LAST_CELL)
            tapCell(fixture, LAST_CELL)
            assertFalse(fixture.view.editorTableSurface.isCellEditMenuVisible)
            awaitDoubleTapTimeout()
            assertFalse(
                "the double tap never opens the menu",
                fixture.view.editorTableSurface.isCellEditMenuVisible
            )
            val input = fixture.view.activeTextInput
            assertTrue("the double tap edits a cell", input !== fixture.root)
            assertEquals(
                fixture.openings()[LAST_CELL].toLong(),
                fixture.view.activeTableCellPosition
            )
        }

    @Test
    fun `selection change blur and editor destroy close the cell menu`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            val openings = fixture.openings()
            showMenuByTappingSelection(fixture, FIRST_CELL, SECOND_CELL)
            fixture.selectCells(openings[FIRST_CELL], openings[LAST_CELL])
            assertFalse(
                "a different rectangle closes the menu",
                fixture.view.editorTableSurface.isCellEditMenuVisible
            )

            fixture.view.editorTableSurface.presentCellEditMenu()
            assertTrue(fixture.view.editorTableSurface.isCellEditMenuVisible)
            fixture.root.clearFocus()
            assertFalse(
                "blur closes the menu",
                fixture.view.editorTableSurface.isCellEditMenuVisible
            )

            assertTrue(fixture.root.requestFocus())
            fixture.view.editorTableSurface.presentCellEditMenu()
            assertTrue(fixture.view.editorTableSurface.isCellEditMenuVisible)
            fixture.view.editorId = 0
            assertFalse(
                "destroy closes the menu",
                fixture.view.editorTableSurface.isCellEditMenuVisible
            )
            assertEquals(null, fixture.root.selectionActionMode)
        }

    private fun rootDragEvent(
        root: EditorEditText,
        action: Int,
        point: Pair<Float, Float>,
        clip: ClipData,
        localState: Any
    ): DragEvent =
        ReflectionHelpers.callStaticMethod<DragEvent>(DragEvent::class.java, "obtain").also {
            ReflectionHelpers.setField(it, "mAction", action)
            ReflectionHelpers.setField(it, "mX", point.first - root.left)
            ReflectionHelpers.setField(it, "mY", point.second - root.top)
            ReflectionHelpers.setField(
                it,
                "mClipData",
                clip.takeIf {
                    action ==
                        DragEvent.ACTION_DROP
                }
            )
            ReflectionHelpers.setField(it, "mClipDescription", clip.description)
            ReflectionHelpers.setField(it, "mLocalState", localState)
        }

    private fun sendDrag(
        root: EditorEditText,
        action: Int,
        point: Pair<Float, Float>,
        clip: ClipData,
        localState: Any
    ): Boolean {
        val event = rootDragEvent(root, action, point, clip, localState)
        return try {
            root.onDragEvent(event)
        } finally {
            ReflectionHelpers.callInstanceMethod<Unit>(event, "recycle")
        }
    }

    private fun startCellDrag(fixture: Fixture, cell: Int): TableCellDragState {
        val drawing = drawing(fixture)
        val (x, y) = cellCenter(fixture, cell)
        ShadowWindowManagerGlobal.clearLastDragClipData()
        return requireNotNull(
            fixture.view.editorTableSurface.startCellDrag(
                x - drawing.left,
                y - drawing.top
            )
        ) {
            "a drag inside the cell selection must lift the cells"
        }
    }

    private fun liftedClip(): ClipData =
        requireNotNull(ShadowWindowManagerGlobal.getLastDragClipData()) {
            "no system drag was started"
        }

    private fun dropCells(fixture: Fixture, state: Any, clip: ClipData, cell: Int): Boolean {
        val target = cellCenter(fixture, cell)
        assertTrue(sendDrag(fixture.root, DragEvent.ACTION_DRAG_STARTED, target, clip, state))
        assertTrue(
            "hovering a real cell is handled",
            sendDrag(fixture.root, DragEvent.ACTION_DRAG_LOCATION, target, clip, state)
        )
        return sendDrag(fixture.root, DragEvent.ACTION_DROP, target, clip, state).also {
            assertEquals(
                "the highlight ends with the drop",
                null,
                drawing(fixture).tableCellDropTarget
            )
        }
    }

    private fun dropTarget(fixture: Fixture, cell: Int) =
        TableCellDropTarget(fixture.adapter.tableRecordsForTesting.keys.single(), cell)

    @Test
    fun `a long press inside the selection lifts the copy flavours as a system drag`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            selectCellsForMenu(fixture, FIRST_CELL, SECOND_CELL)
            assertTrue(fixture.root.onTextContextMenuItem(android.R.id.copy))
            val copied = requireNotNull(clipboard().primaryClip)
            ShadowWindowManagerGlobal.clearLastDragClipData()
            val center = cellCenter(fixture, FIRST_CELL)

            val down = MotionEvent.obtain(
                0,
                0,
                MotionEvent.ACTION_DOWN,
                center.first,
                center.second,
                0
            )
            try {
                fixture.view.editorContentFrame.dispatchTouchEvent(down)
            } finally {
                down.recycle()
            }
            assertEquals(
                "nothing lifts before the long-press timeout",
                null,
                ShadowWindowManagerGlobal.getLastDragClipData()
            )
            shadowOf(
                Looper.getMainLooper()
            ).idleFor(Duration.ofMillis(ViewConfiguration.getLongPressTimeout().toLong()))
            val up = MotionEvent.obtain(
                0,
                ViewConfiguration.getLongPressTimeout().toLong(),
                MotionEvent.ACTION_UP,
                center.first,
                center.second,
                0
            )
            try {
                fixture.view.editorContentFrame.dispatchTouchEvent(up)
            } finally {
                up.recycle()
            }
            awaitDoubleTapTimeout()

            val lifted = liftedClip()
            assertEquals(FIRST_ROW_TSV, lifted.getItemAt(0).text.toString())
            assertEquals(copied.getItemAt(0).htmlText, lifted.getItemAt(0).htmlText)
            assertEquals(
                copied.description.extras?.getString(EditorClipboard.EXTRA_FRAGMENT),
                lifted.description.extras?.getString(EditorClipboard.EXTRA_FRAGMENT)
            )
            assertTrue(lifted.description.hasMimeType(EditorClipboard.MIME_TYPE_FRAGMENT))
            assertFalse(
                "the lifting press is not a menu tap",
                fixture.view.editorTableSurface.isCellEditMenuVisible
            )
            assertEquals("lifting never mutates", emptyList<String>(), fixture.backend.mutations)
            assertEquals(0, fixture.updates.size)
        }

    @Test
    fun `a handle press or a quick tap never lifts the cells`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            selectCellsForMenu(fixture, FIRST_CELL, SECOND_CELL)
            ShadowWindowManagerGlobal.clearLastDragClipData()
            val drawing = drawing(fixture)
            val head = drawing.selectionHandles().single {
                it.role == TableSelectionHandleRole.HEAD
            }
            val handle = head.x + drawing.left to head.y + drawing.top
            val down = MotionEvent.obtain(
                0,
                0,
                MotionEvent.ACTION_DOWN,
                handle.first,
                handle.second,
                0
            )
            try {
                fixture.view.editorContentFrame.dispatchTouchEvent(down)
            } finally {
                down.recycle()
            }
            shadowOf(
                Looper.getMainLooper()
            ).idleFor(Duration.ofMillis(ViewConfiguration.getLongPressTimeout().toLong()))
            assertEquals(
                "the handle keeps precedence",
                null,
                ShadowWindowManagerGlobal.getLastDragClipData()
            )
            fixture.view.editorTableSurface.cancelActiveDrag()

            tapCell(fixture, FIRST_CELL)
            shadowOf(
                Looper.getMainLooper()
            ).idleFor(Duration.ofMillis(ViewConfiguration.getLongPressTimeout().toLong()))
            assertEquals(
                "a tap is not a long press",
                null,
                ShadowWindowManagerGlobal.getLastDragClipData()
            )
            assertTrue(
                "the tap still toggles the menu",
                fixture.view.editorTableSurface.isCellEditMenuVisible
            )
        }

    @Test
    fun `the drag shadow is the union of the selected cells`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            selectCellsForMenu(fixture, FIRST_CELL, SECOND_CELL)
            val drawing = drawing(fixture)
            val rects =
                requireNotNull(
                    drawing.selectedTableCellRects(
                        requireNotNull(drawing.selectedTableCellEndpoints).first
                    )
                )
            val union = RectF(rects.first()).apply { rects.drop(1).forEach(::union) }
            val shadow = TableCellDragShadow(
                drawing,
                rects,
                union.left + SHADOW_TOUCH_INSET,
                union.top + SHADOW_TOUCH_INSET
            )
            val size = Point()
            val touch = Point()

            shadow.onProvideShadowMetrics(size, touch)

            assertEquals(ceil(union.width()).toInt(), size.x)
            assertEquals(ceil(union.height()).toInt(), size.y)
            assertEquals(Point(SHADOW_TOUCH_INSET.toInt(), SHADOW_TOUCH_INSET.toInt()), touch)
        }

    @Test
    fun `a same editor drop moves the cells in one undoable mutation`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            selectCellsForMenu(fixture, FIRST_CELL, SECOND_CELL)
            val before = fixture.adapter.documentJson()
            val state = startCellDrag(fixture, FIRST_CELL)
            assertTrue(state.movable)
            val target = cellCenter(fixture, THIRD_CELL)
            val clip = liftedClip()
            assertTrue(sendDrag(fixture.root, DragEvent.ACTION_DRAG_STARTED, target, clip, state))
            assertTrue(sendDrag(fixture.root, DragEvent.ACTION_DRAG_LOCATION, target, clip, state))
            assertEquals(
                "hovering highlights the real drop cell",
                dropTarget(fixture, THIRD_CELL),
                drawing(fixture).tableCellDropTarget
            )

            assertTrue(sendDrag(fixture.root, DragEvent.ACTION_DROP, target, clip, state))

            assertEquals(null, drawing(fixture).tableCellDropTarget)
            assertEquals(listOf(listOf("", ""), listOf("A", "B")), fixture.cellTexts())
            assertOneUndoableMutation(fixture, PASTE_COMMAND, before)
        }

    @Test
    fun `dropping the cells onto themselves is a no op`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            selectCellsForMenu(fixture, FIRST_CELL, SECOND_CELL)
            val before = fixture.adapter.documentJson()
            val state = startCellDrag(fixture, FIRST_CELL)
            val target = cellCenter(fixture, SECOND_CELL)
            val clip = liftedClip()
            assertTrue(sendDrag(fixture.root, DragEvent.ACTION_DRAG_STARTED, target, clip, state))
            assertTrue(sendDrag(fixture.root, DragEvent.ACTION_DRAG_LOCATION, target, clip, state))
            assertEquals(
                "a self drop highlights nothing",
                null,
                drawing(fixture).tableCellDropTarget
            )

            assertFalse(sendDrag(fixture.root, DragEvent.ACTION_DROP, target, clip, state))

            assertEquals(emptyList<String>(), fixture.backend.mutations)
            assertEquals(before, fixture.adapter.documentJson())
            assertFalse(requireNotNull(fixture.adapter.historyCanUndo()))
        }

    @Test
    fun `a drop from another editor copies and leaves the source intact`() =
        withTable(GRID_DOCUMENT, attached = true) { source ->
            selectCellsForMenu(source, FIRST_CELL, SECOND_CELL)
            val sourceBefore = source.adapter.documentJson()
            val state = startCellDrag(source, FIRST_CELL)
            val clip = liftedClip()
            withTable(TARGET_GRID_DOCUMENT, attached = true) { target ->
                target.relayout()
                val before = target.adapter.documentJson()

                assertTrue(dropCells(target, state, clip, THIRD_CELL))

                assertEquals(listOf(listOf("w", "x"), listOf("A", "B")), target.cellTexts())
                assertOneUndoableMutation(target, PASTE_COMMAND, before)
            }
            assertEquals(
                "a copy never clears the source",
                sourceBefore,
                source.adapter.documentJson()
            )
            assertEquals(emptyList<String>(), source.backend.mutations)
        }

    @Test
    fun `an external drop pastes its clip as a matrix at the real drop cell`() =
        withTable(TARGET_GRID_DOCUMENT, attached = true) { fixture ->
            fixture.relayout()
            val before = fixture.adapter.documentJson()

            assertTrue(
                dropCells(
                    fixture,
                    Any(),
                    ClipData.newPlainText(STALE_LABEL, EXTERNAL_TSV),
                    LAST_CELL
                )
            )

            assertEquals(listOf(listOf("w", "x", ""), listOf("y", "e1", "e2")), fixture.cellTexts())
            assertOneUndoableMutation(fixture, PASTE_COMMAND, before)
        }

    @Test
    fun `a cell drag dropped in prose keeps the text drop and leaves the source`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            selectCellsForMenu(fixture, FIRST_CELL, SECOND_CELL)
            val state = startCellDrag(fixture, FIRST_CELL)
            val clip = liftedClip()
            val prose = proseStart(fixture)

            assertTrue(sendDrag(fixture.root, DragEvent.ACTION_DRAG_STARTED, prose, clip, state))
            assertTrue(sendDrag(fixture.root, DragEvent.ACTION_DRAG_LOCATION, prose, clip, state))
            assertEquals(null, drawing(fixture).tableCellDropTarget)
            assertTrue(sendDrag(fixture.root, DragEvent.ACTION_DROP, prose, clip, state))

            assertEquals(listOf(listOf("A", "B"), listOf("C", "D")), fixture.cellTexts())
            assertTrue(
                "the plain text is inserted into the prose",
                requireNotNull(fixture.adapter.documentHtml()).contains(FIRST_ROW_TSV + AFTER_TEXT)
            )
        }

    @Test
    fun `a text drag in the prose of a root table document moves the text`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            fixture.relayout()
            val text = requireNotNull(fixture.root.text).toString()
            val start = text.indexOf(AFTER_TEXT)
            fixture.root.setSelection(start, start + MOVED_PREFIX.length)
            val end = start + AFTER_TEXT.length
            val clip = ClipData.newPlainText(STALE_LABEL, MOVED_PREFIX)

            assertTrue(
                "a root table no longer disables text drags",
                sendDrag(
                    fixture.root,
                    DragEvent.ACTION_DRAG_STARTED,
                    proseOffset(fixture, end),
                    clip,
                    fixture.root
                )
            )
            assertTrue(
                sendDrag(
                    fixture.root,
                    DragEvent.ACTION_DROP,
                    proseOffset(fixture, end),
                    clip,
                    fixture.root
                )
            )

            assertTrue(
                requireNotNull(fixture.adapter.documentHtml()).contains("<p>ter$MOVED_PREFIX</p>")
            )
            assertEquals(
                "the table is untouched",
                listOf(listOf("A", "B"), listOf("C", "D")),
                fixture.cellTexts()
            )
        }

    @Test
    fun `a read only editor lifts a copy only drag and refuses drops`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            selectCellsForMenu(fixture, FIRST_CELL, SECOND_CELL)
            fixture.root.isEditable = false
            val before = fixture.adapter.documentJson()

            val state = startCellDrag(fixture, FIRST_CELL)
            assertFalse("a read-only source can only be copied", state.movable)
            val target = cellCenter(fixture, THIRD_CELL)
            val clip = liftedClip()
            assertTrue(sendDrag(fixture.root, DragEvent.ACTION_DRAG_LOCATION, target, clip, state))
            assertEquals(null, drawing(fixture).tableCellDropTarget)
            assertFalse(sendDrag(fixture.root, DragEvent.ACTION_DROP, target, clip, state))

            assertEquals(emptyList<String>(), fixture.backend.mutations)
            assertEquals(before, fixture.adapter.documentJson())
        }

    @Test
    fun `a view that does not own the table cannot move or receive cell drops`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            val openings = fixture.openings()
            fixture.selectCells(openings[FIRST_CELL], openings[SECOND_CELL])
            val stale = fixture.nonOwnerView()
            (fixture.view.parent as FrameLayout).addView(
                stale,
                FrameLayout.LayoutParams(VIEW_WIDTH, VIEW_HEIGHT)
            )
            fixture.relayout(stale)
            assertTrue(stale.editorEditText.requestFocus())
            val staleDrawing = stale.editorTableSurface.drawingView
            val presented = staleDrawing.presentedTableCells().single {
                it.sourceIndex == FIRST_CELL
            }
            val before = fixture.adapter.documentJson()

            val state =
                requireNotNull(
                    stale.editorTableSurface.startCellDrag(
                        presented.bounds.centerX(),
                        presented.bounds.centerY()
                    )
                )
            assertFalse("a non-owner can only copy", state.movable)
            val target = staleDrawing.presentedTableCells().single { it.sourceIndex == THIRD_CELL }
            assertFalse(
                sendDrag(
                    stale.editorEditText,
                    DragEvent.ACTION_DROP,
                    target.bounds.centerX() + staleDrawing.left to
                        target.bounds.centerY() + staleDrawing.top,
                    liftedClip(),
                    state
                )
            )

            assertEquals(emptyList<String>(), fixture.backend.mutations)
            assertEquals(before, fixture.adapter.documentJson())
        }

    @Test
    fun `a drop onto a synthetic slot of an irregular table is refused without mutation`() =
        withTable(IRREGULAR_DOCUMENT, attached = true) { fixture ->
            selectCellsForMenu(fixture, IRREGULAR_LATER_CELL, IRREGULAR_LATER_CELL)
            val state = startCellDrag(fixture, IRREGULAR_LATER_CELL)
            val clip = liftedClip()
            val drawing = drawing(fixture)
            val openings = fixture.openings()
            val wide = drawing.presentedTableCells().single {
                it.sourceIndex == IRREGULAR_WIDE_CELL
            }
            val later = drawing.presentedTableCells().single {
                it.sourceIndex ==
                    IRREGULAR_LATER_CELL
            }
            val gap =
                (later.bounds.right + wide.bounds.right) / 2f + drawing.left to
                    later.bounds.centerY() + drawing.top
            assertTrue(
                "the gap lies inside the table",
                drawing.hasTableAt(
                    gap.first - drawing.left,
                    gap.second - drawing.top
                )
            )
            assertTrue(
                "the gap holds no real cell",
                drawing.presentedTableCells().none {
                    it.bounds.contains(gap.first - drawing.left, gap.second - drawing.top)
                }
            )
            val before = fixture.adapter.documentJson()

            for (localState in listOf(state, Any())) {
                assertTrue(
                    sendDrag(fixture.root, DragEvent.ACTION_DRAG_STARTED, gap, clip, localState)
                )
                assertTrue(
                    "the table claims the hover",
                    sendDrag(fixture.root, DragEvent.ACTION_DRAG_LOCATION, gap, clip, localState)
                )
                assertEquals(
                    "a synthetic slot is never highlighted",
                    null,
                    drawing.tableCellDropTarget
                )
                assertFalse(sendDrag(fixture.root, DragEvent.ACTION_DROP, gap, clip, localState))
            }

            assertEquals(emptyList<String>(), fixture.backend.mutations)
            assertEquals(0, fixture.updates.size)
            assertEquals("the document is byte-identical", before, fixture.adapter.documentJson())
        }

    @Test
    fun `a move is refused when the document changed after the drag started`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            selectCellsForMenu(fixture, FIRST_CELL, SECOND_CELL)
            val state = startCellDrag(fixture, FIRST_CELL)
            assertTrue(state.movable)
            val clip = liftedClip()
            val edit =
                requireNotNull(
                    fixture.adapter.scalarPositionForDoc(
                        fixture.openings()[LAST_CELL] + CELL_TEXT_OFFSET
                    )
                )
            assertTrue(
                fixture.root.applyUpdateJSON(
                    requireNotNull(fixture.adapter.replaceTextRange(edit, edit, STALE_EDIT))
                )
            )
            assertEquals(
                listOf(listOf("A", "B"), listOf("C", STALE_EDIT + "D")),
                fixture.cellTexts()
            )
            fixture.relayout()
            fixture.backend.mutations.clear()
            val before = fixture.adapter.documentJson()
            val target = cellCenter(fixture, THIRD_CELL)

            assertTrue(sendDrag(fixture.root, DragEvent.ACTION_DRAG_LOCATION, target, clip, state))
            assertEquals("a stale move is not offered", null, drawing(fixture).tableCellDropTarget)
            assertFalse(sendDrag(fixture.root, DragEvent.ACTION_DROP, target, clip, state))

            assertEquals(emptyList<String>(), fixture.backend.mutations)
            assertEquals(
                "a stale move neither moves nor degrades to a copy",
                before,
                fixture.adapter.documentJson()
            )
        }

    @Test
    fun `a composing editor refuses to lift its cells`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            fixture.relayout()
            val text = requireNotNull(fixture.root.text).toString()
            fixture.root.setSelection(text.indexOf(AFTER_TEXT))
            fixture.root.beginExternalTextComposition(COMPOSITION_SESSION)
            assertTrue(fixture.root.hasPendingCompositionForExternalRefresh())
            selectCellsForMenu(fixture, FIRST_CELL, SECOND_CELL)
            assertTrue(
                "the composition outlives the cell selection",
                fixture.root.hasPendingCompositionForExternalRefresh()
            )
            val drawing = drawing(fixture)
            val (x, y) = cellCenter(fixture, FIRST_CELL)
            ShadowWindowManagerGlobal.clearLastDragClipData()

            assertEquals(
                null,
                fixture.view.editorTableSurface.startCellDrag(
                    x - drawing.left,
                    y - drawing.top
                )
            )
            assertEquals(
                "no system drag starts",
                null,
                ShadowWindowManagerGlobal.getLastDragClipData()
            )
        }

    @Test
    fun `an active composition refuses a cell drop`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            fixture.relayout()
            val text = requireNotNull(fixture.root.text).toString()
            fixture.root.setSelection(text.indexOf(AFTER_TEXT))
            fixture.root.beginExternalTextComposition(COMPOSITION_SESSION)
            assertTrue(fixture.root.hasPendingCompositionForExternalRefresh())
            val before = fixture.adapter.documentJson()
            val clip = ClipData.newPlainText(STALE_LABEL, EXTERNAL_TSV)

            assertTrue(
                sendDrag(
                    fixture.root,
                    DragEvent.ACTION_DRAG_LOCATION,
                    cellCenter(fixture, THIRD_CELL),
                    clip,
                    Any()
                )
            )
            assertEquals(null, drawing(fixture).tableCellDropTarget)
            assertFalse(
                sendDrag(
                    fixture.root,
                    DragEvent.ACTION_DROP,
                    cellCenter(fixture, THIRD_CELL),
                    clip,
                    Any()
                )
            )

            assertEquals(before, fixture.adapter.documentJson())
            assertEquals(
                emptyList<String>(),
                fixture.backend.mutations.filter {
                    it.startsWith(APPLY_COMMAND)
                }
            )
        }

    private fun proseOffset(fixture: Fixture, offset: Int): Pair<Float, Float> {
        val root = fixture.root
        val layout = requireNotNull(root.layout)
        val line = layout.getLineForOffset(offset)
        return layout.getPrimaryHorizontal(offset) + root.totalPaddingLeft + root.left to
            layout.editorTextLineTop(line).toFloat() + root.totalPaddingTop + root.top +
            PROSE_LINE_INSET
    }

    private fun proseStart(fixture: Fixture): Pair<Float, Float> =
        proseOffset(fixture, requireNotNull(fixture.root.text).toString().indexOf(AFTER_TEXT))

    private companion object {
        const val APPLY_COMMAND = "applyCommand"
        const val STALE_EDIT = "!"
        const val IRREGULAR_WIDE_CELL = 1
        const val IRREGULAR_LATER_CELL = 2
        const val IRREGULAR_DOCUMENT = """{"type":"doc","content":[{"type":"table",""" +
            """"content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell","attrs":{"rowspan":2,""" +
            """"colwidth":[100]},"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"tall"}]}]},""" +
            """{"type":"table_cell","attrs":{"colspan":2,""" +
            """"colwidth":[100,100]},"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"wide"}]}]}]},""" +
            """{"type":"table_row","content":[{"type":"table_cell",""" +
            """"attrs":{"colwidth":[100]},""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"later"}]}]}]}]}]}"""
        const val EXTERNAL_TSV = "e1\te2"
        const val MOVED_PREFIX = "af"
        const val COMPOSITION_SESSION = "cell-drag-composition"
        const val SHADOW_TOUCH_INSET = 4f
        const val PROSE_LINE_INSET = 1f
        const val TARGET_GRID_DOCUMENT = """{"type":"doc","content":[{"type":"table",""" +
            """"content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell",""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"w"}]}]},""" +
            """{"type":"table_cell","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"x"}]}]}]},""" +
            """{"type":"table_row","content":[{"type":"table_cell",""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"y"}]}]},""" +
            """{"type":"table_cell","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"z"}]}]}]}]},""" +
            """{"type":"paragraph","content":[{"type":"text",""" +
            """"text":"after"}]}]}"""
        const val APPLY_INPUT = "applyInput"
        const val SET_SELECTION = "setSelection"
        const val PASTE_COMMAND = "paste"
        const val DELETE_BACKWARD_COMMAND = "deleteBackward"
        const val CELL_SELECTION = "cell"
        const val TOUCH_STEP_MS = 20L
        const val AFTER_TEXT = "after"
        const val IMAGE_MIME_TYPE = "image/png"
        const val CONTENT_URI = "content://com.apollohg.editor.test/image"
        const val OUTSIDE_TOUCH_OFFSET = 200f
        val CELL_MENU_ITEMS = listOf(android.R.id.cut, android.R.id.copy, android.R.id.paste)
        const val TEXT_SELECTION = "text"
        const val EMITTED_ERROR_NOTE = "emit "
        const val CELL_TEXT_OFFSET = 2
        const val TABLE_NODE = "table"
        const val TEXT_NODE = "text"
        const val VIEW_WIDTH = 900
        const val VIEW_HEIGHT = 500
        const val OUTER_TABLE = 0
        const val NESTED_TABLE = 1
        const val FIRST_CELL = 0
        const val SECOND_CELL = 1
        const val THIRD_CELL = 2
        const val LAST_CELL = 3
        const val MERGED_WIDE_CELL = 0
        const val MERGED_SECOND_ROW_MIDDLE_CELL = 3
        const val MERGED_COLSPAN = 2
        const val STALE_LABEL = "stale"
        const val STALE_TEXT = "stale clipboard"
        const val FIRST_ROW_TSV = "A\tB"
        const val PASTED_GRID_TSV = "w\tx\ny\tz"
        const val PLAIN_ALTERNATIVE_TSV = "p1\tp2"
        const val HTML_TABLE = "<table><tr><td>h1</td><td>h2</td></tr></table>"
        const val NESTED_CELL_TEXT = "Nested"
        const val ATOM_METADATA_KIND = "person"
        const val MERGED_RECTANGLE_TSV = "wide\t\nc0\tc1"
        const val MERGED_RECTANGLE_HTML =
            "<table><tbody><tr><td colspan=\"2\" rowspan=\"1\"><p>wide</p></td></tr>" +
                "<tr><td colspan=\"1\" rowspan=\"1\"><p>c0</p></td><td colspan=\"1\" rowspan=\"1\"><p>c1</p></td></tr></tbody></table>"

        const val TABLE_CONFIG = """{"schema":{"nodes":[{"name":"doc","content":"block+",""" +
            """"role":"doc"},{"name":"paragraph","content":"inline*",""" +
            """"group":"block","role":"textBlock","htmlTag":"p"},""" +
            """{"name":"text","content":"","group":"inline",""" +
            """"role":"text"},{"name":"table","content":"table_row+",""" +
            """"group":"block","role":"block","tableRole":"table",""" +
            """"htmlTag":"table"},{"name":"table_row",""" +
            """"content":"(table_cell | table_header)*",""" +
            """"role":"block","tableRole":"row","htmlTag":"tr"},""" +
            """{"name":"table_cell","content":"block+","role":"block",""" +
            """"tableRole":"cell","htmlTag":"td",""" +
            """"attrs":{"colspan":{"type":"number","default":1,""" +
            """"min":1},"rowspan":{"type":"number","default":1,""" +
            """"min":1},"colwidth":{"default":null}}},""" +
            """{"name":"table_header","content":"block+",""" +
            """"role":"block","tableRole":"header_cell",""" +
            """"htmlTag":"th","attrs":{"colspan":{"type":"number",""" +
            """"default":1,"min":1},"rowspan":{"type":"number",""" +
            """"default":1,"min":1},"colwidth":{"default":null}}}],""" +
            """"marks":[]},"initialization":{"type":"localEmpty"}}"""
        val ATOM_TABLE_CONFIG = TABLE_CONFIG.replace(
            """{"name":"text","content":"","group":"inline","role":"text"}""",
            """{"name":"text","content":"","group":"inline",""" +
                """"role":"text"},{"name":"mention","role":"inline",""" +
                """"group":"inline","isVoid":true,"attrs":{"id":{},""" +
                """"label":{"default":""}},"allowUndeclaredAttrs":true}"""
        )
        const val GRID_DOCUMENT = """{"type":"doc","content":[{"type":"table",""" +
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
            """"content":[{"type":"text","text":"D"}]}]}]}]},""" +
            """{"type":"paragraph","content":[{"type":"text",""" +
            """"text":"after"}]}]}"""
        const val MERGED_DOCUMENT = """{"type":"doc","content":[{"type":"table",""" +
            """"content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell","attrs":{"colspan":2},""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"wide"}]}]},""" +
            """{"type":"table_cell","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"right"}]}]}]},""" +
            """{"type":"table_row","content":[{"type":"table_cell",""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"c0"}]}]},""" +
            """{"type":"table_cell","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"c1"}]}]},""" +
            """{"type":"table_cell","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"c2"}]}]}]}]}]}"""
        const val ATOM_DOCUMENT = """{"type":"doc","content":[{"type":"table",""" +
            """"content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell",""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"a"},""" +
            """{"type":"mention","attrs":{"id":"m1","label":"Sam",""" +
            """"metadata":{"kind":"person"}}}]}]},""" +
            """{"type":"table_cell","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"b"}]}]}]},""" +
            """{"type":"table_row","content":[{"type":"table_cell",""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"c"}]}]},""" +
            """{"type":"table_cell","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"d"}]}]}]}]}]}"""
    }
}
