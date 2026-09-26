package com.apollohg.editor

import android.app.Activity
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.net.Uri
import android.os.Looper
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.widget.FrameLayout
import java.time.Duration
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
import com.apollohg.editor.viewer.PreparedProseDrawingView
import com.apollohg.editor.viewer.TableSelectionHandleRole

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], qualifiers = "w960dp-h640dp")
internal class EditorTableClipboardTest {
    private class RecordingBackend : EditorV2Backend by UniffiEditorV2Backend {
        val mutations = mutableListOf<String>()

        override fun applyCommand(editorId: String, requestJson: String): EditorV2CallResult<String> {
            mutations += "$APPLY_COMMAND:" +
                JSONObject(requestJson).getJSONObject("command").getString("type")
            return UniffiEditorV2Backend.applyCommand(editorId, requestJson)
        }

        override fun applyInput(editorId: String, requestJson: String): EditorV2CallResult<String> {
            mutations += APPLY_INPUT
            return UniffiEditorV2Backend.applyInput(editorId, requestJson)
        }

        override fun setSelection(editorId: String, requestJson: String): EditorV2CallResult<String> {
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
            val table = adapter.cachedTableRecords.values.sortedBy { it.getInt("tablePos") }[tableIndex]
            val cells = table.getJSONArray("cells")
            return (0 until cells.length()).map { cells.getJSONObject(it).getInt("sourcePos") }
        }

        fun selectCells(anchor: Int, head: Int) {
            fun point(opening: Int) = JSONObject().put("kind", "document").put("offset", opening)
            select(
                JSONObject().put("type", CELL_SELECTION)
                    .put("anchorCell", point(anchor)).put("headCell", point(head))
            )
            assertTrue("root did not adopt the cell selection", root.authoritativeCellSelectionActive)
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
                View.MeasureSpec.makeMeasureSpec(target.editorEditText.width, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(target.editorEditText.height, View.MeasureSpec.EXACTLY)
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
            assertTrue("engine rejected the selection: $admitted", admitted is EditorV2CallResult.Ok)
            assertTrue(root.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
        }

        fun nonOwnerView(): RichTextEditorView {
            val view = RichTextEditorView(RuntimeEnvironment.getApplication())
            view.editorId = token
            assertTrue(view.editorEditText.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
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
            return (0 until content.length()).joinToString("") { cellText(content.getJSONObject(it)) }
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
            val activity = if (attached) Robolectric.buildActivity(Activity::class.java).setup() else null
            val view = RichTextEditorView(activity?.get() ?: RuntimeEnvironment.getApplication())
            activity?.get()?.setContentView(FrameLayout(activity.get()).apply {
                addView(view, FrameLayout.LayoutParams(VIEW_WIDTH, VIEW_HEIGHT))
            })
            view.editorId = token
            assertTrue(view.editorEditText.applyUpdateJSON(requireNotNull(adapter.setContentJson(document))))
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
            assertFalse("fixture must start without history", requireNotNull(adapter.historyCanUndo()))
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

            assertEquals(FIRST_ROW_TSV, requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString())
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
                        arrayOf(ClipDescription.MIMETYPE_TEXT_HTML, ClipDescription.MIMETYPE_TEXT_PLAIN)
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
            assertEquals(NESTED_CELL_TEXT, requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString())
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
                fixture.adapter.debugNotes.drop(notesBefore).filter { it.startsWith(EMITTED_ERROR_NOTE) }
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
            assertTrue("the stale view adopted the cell selection", stale.authoritativeCellSelectionActive)
            val before = fixture.adapter.documentJson()
            clipboard().setPrimaryClip(ClipData.newPlainText(STALE_LABEL, PASTED_GRID_TSV))

            assertTrue(stale.onTextContextMenuItem(android.R.id.cut))
            assertTrue(stale.dispatchKeyEvent(shortcut(KeyEvent.KEYCODE_V)))

            assertEquals(emptyList<String>(), fixture.backend.mutations)
            assertEquals(PASTED_GRID_TSV, requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString())
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
                    ?.getString(EditorClipboard.EXTRA_FRAGMENT).orEmpty().contains(ATOM_METADATA_KIND)
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

    private fun drawing(fixture: Fixture): PreparedProseDrawingView = fixture.view.editorTableSurface.drawingView

    private fun cellCenter(fixture: Fixture, cell: Int): Pair<Float, Float> {
        val drawing = drawing(fixture)
        val opening = fixture.openings()[cell]
        val presented = drawing.presentedTableCells().single { it.sourcePosition == opening }
        return presented.bounds.centerX() + drawing.left to presented.bounds.centerY() + drawing.top
    }

    private fun dispatchFrameTouches(fixture: Fixture, points: List<Pair<Int, Pair<Float, Float>>>) {
        points.forEachIndexed { index, (action, point) ->
            val event = MotionEvent.obtain(0, TOUCH_STEP_MS * index, action, point.first, point.second, 0)
            try { fixture.view.editorContentFrame.dispatchTouchEvent(event) } finally { event.recycle() }
        }
    }

    private fun dragHead(fixture: Fixture, toCell: Int) {
        val drawing = drawing(fixture)
        val head = drawing.selectionHandles().single { it.role == TableSelectionHandleRole.HEAD }
        val target = cellCenter(fixture, toCell)
        dispatchFrameTouches(fixture, listOf(
            MotionEvent.ACTION_DOWN to (head.x + drawing.left to head.y + drawing.top),
            MotionEvent.ACTION_MOVE to target,
            MotionEvent.ACTION_UP to target
        ))
        assertEquals("the drag must land on the target cell",
            fixture.openings()[toCell], fixture.engineSelection().getInt("headCell"))
    }

    private fun awaitDoubleTapTimeout() =
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(ViewConfiguration.getDoubleTapTimeout().toLong()))

    private fun tapCell(fixture: Fixture, cell: Int) {
        val center = cellCenter(fixture, cell)
        dispatchFrameTouches(fixture, listOf(MotionEvent.ACTION_DOWN to center, MotionEvent.ACTION_UP to center))
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
        assertTrue("a tap inside the selection shows the cell menu",
            fixture.view.editorTableSurface.isCellEditMenuVisible)
        assertTrue("the tap keeps the cell selection", fixture.root.authoritativeCellSelectionActive)
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
        assertFalse("an item closes the menu", fixture.view.editorTableSurface.isCellEditMenuVisible)
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
            assertTrue("the cell menu replaces the text menu", fixture.view.editorTableSurface.isCellEditMenuVisible)
            assertTrue(fixture.root.selectionActionMode !== textMenu)
            assertEquals(expected, menuItemIds(fixture))

            fixture.root.interaction.startSelectionActionMode()
            assertFalse("a text menu closes the cell menu", fixture.view.editorTableSurface.isCellEditMenuVisible)
            assertTrue(requireNotNull(fixture.root.selectionActionMode).tag === TextSelectionActionMode)
        }

    @Test
    fun `handle drag end keeps the menu closed and a tap inside shows cut copy and paste`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            clipboard().setPrimaryClip(ClipData.newPlainText(STALE_LABEL, PASTED_GRID_TSV))
            selectCellsForMenu(fixture, FIRST_CELL, FIRST_CELL)
            dragHead(fixture, LAST_CELL)
            assertFalse("a handle drag leaves the toolbar in charge",
                fixture.view.editorTableSurface.isCellEditMenuVisible)
            assertEquals(null, fixture.root.selectionActionMode)

            tapCell(fixture, FIRST_CELL)
            assertFalse("the menu waits for a possible double tap",
                fixture.view.editorTableSurface.isCellEditMenuVisible)
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
            assertEquals(FIRST_ROW_TSV, requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString())
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
            assertEquals(FIRST_ROW_TSV, requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString())
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
            clipboard().setPrimaryClip(ClipData(ClipDescription(STALE_LABEL, arrayOf(IMAGE_MIME_TYPE)),
                ClipData.Item(Uri.parse(CONTENT_URI))))
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
            val selected = requireNotNull(drawing.selectedTableCellRects(
                drawing.selectedTableCellSourcePositions.keys.single())).single()
            val center = selected.centerX() + drawing.left to selected.centerY() + drawing.top
            dispatchFrameTouches(fixture, listOf(MotionEvent.ACTION_DOWN to center, MotionEvent.ACTION_UP to center))
            awaitDoubleTapTimeout()
            assertTrue("a tap inside the read-only selection shows the menu",
                fixture.view.editorTableSurface.isCellEditMenuVisible)
            assertEquals(listOf(android.R.id.copy), menuItemIds(fixture))

            clickMenuItem(fixture, android.R.id.copy)

            assertEquals(NESTED_CELL_TEXT, requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString())
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
            (fixture.view.parent as FrameLayout).addView(stale, FrameLayout.LayoutParams(VIEW_WIDTH, VIEW_HEIGHT))
            fixture.relayout(stale)
            assertTrue(stale.editorEditText.requestFocus())
            assertTrue(stale.editorEditText.authoritativeCellSelectionActive)

            stale.editorTableSurface.presentCellEditMenu()

            assertTrue(stale.editorTableSurface.isCellEditMenuVisible)
            val mode = requireNotNull(stale.editorEditText.selectionActionMode)
            assertEquals(listOf(android.R.id.copy), (0 until mode.menu.size()).map { mode.menu.getItem(it).itemId })
            assertTrue(mode.menu.performIdentifierAction(android.R.id.copy, 0))
            assertEquals(FIRST_ROW_TSV + "\n" + "C\tD", requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString())
            assertEquals("a non-owner copy never mutates", emptyList<String>(), fixture.backend.mutations)
        }

    @Test
    fun `a text action mode is replaced by the full cell menu when the clipboard has text`() =
        assertCellMenuReplacesTextMenu(ClipData.newPlainText(STALE_LABEL, PASTED_GRID_TSV), CELL_MENU_ITEMS)

    @Test
    fun `a text action mode is replaced by a cell menu without paste when the clipboard is empty`() =
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
            assertFalse("a touch outside the selection closes the menu",
                fixture.view.editorTableSurface.isCellEditMenuVisible)
            assertTrue("the outside tap edits that cell", fixture.view.activeTextInput !== fixture.root)
            assertEquals(emptyList<String>(), fixture.backend.mutations.filter { it.startsWith(APPLY_COMMAND) })
        }

    @Test
    fun `a touch outside the table closes the menu`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            showMenuByTappingSelection(fixture, FIRST_CELL, SECOND_CELL)
            val drawing = drawing(fixture)
            val below = drawing.selectedTableCellRects(requireNotNull(drawing.selectedTableCellEndpoints).first)
                .orEmpty().maxOf { it.bottom } + drawing.top + OUTSIDE_TOUCH_OFFSET
            dispatchFrameTouches(fixture, listOf(MotionEvent.ACTION_DOWN to (cellCenter(fixture, FIRST_CELL).first to below)))
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
            assertFalse("the double tap never opens the menu", fixture.view.editorTableSurface.isCellEditMenuVisible)
            val input = fixture.view.activeTextInput
            assertTrue("the double tap edits a cell", input !== fixture.root)
            assertEquals(fixture.openings()[LAST_CELL].toLong(),
                requireNotNull(input.tableCellPositionMap).binding.cellSourcePos)
        }

    @Test
    fun `selection change blur and editor destroy close the cell menu`() =
        withTable(GRID_DOCUMENT, attached = true) { fixture ->
            val openings = fixture.openings()
            showMenuByTappingSelection(fixture, FIRST_CELL, SECOND_CELL)
            fixture.selectCells(openings[FIRST_CELL], openings[LAST_CELL])
            assertFalse("a different rectangle closes the menu", fixture.view.editorTableSurface.isCellEditMenuVisible)

            fixture.view.editorTableSurface.presentCellEditMenu()
            assertTrue(fixture.view.editorTableSurface.isCellEditMenuVisible)
            fixture.root.clearFocus()
            assertFalse("blur closes the menu", fixture.view.editorTableSurface.isCellEditMenuVisible)

            assertTrue(fixture.root.requestFocus())
            fixture.view.editorTableSurface.presentCellEditMenu()
            assertTrue(fixture.view.editorTableSurface.isCellEditMenuVisible)
            fixture.view.editorId = 0
            assertFalse("destroy closes the menu", fixture.view.editorTableSurface.isCellEditMenuVisible)
            assertEquals(null, fixture.root.selectionActionMode)
        }

    private companion object {
        const val APPLY_COMMAND = "applyCommand"
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
        const val MERGED_RECTANGLE_HTML = "<table><tbody><tr><td colspan=\"2\" rowspan=\"1\"><p>wide</p></td></tr>" +
            "<tr><td colspan=\"1\" rowspan=\"1\"><p>c0</p></td><td colspan=\"1\" rowspan=\"1\"><p>c1</p></td></tr></tbody></table>"

        const val TABLE_CONFIG = """{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock","htmlTag":"p"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table","htmlTag":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row","htmlTag":"tr"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","htmlTag":"td","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","htmlTag":"th","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"""
        val ATOM_TABLE_CONFIG = TABLE_CONFIG.replace(
            """{"name":"text","content":"","group":"inline","role":"text"}""",
            """{"name":"text","content":"","group":"inline","role":"text"},{"name":"mention","role":"inline","group":"inline","isVoid":true,"attrs":{"id":{},"label":{"default":""}},"allowUndeclaredAttrs":true}"""
        )
        const val GRID_DOCUMENT = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"A"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"B"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"C"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"D"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"""
        const val MERGED_DOCUMENT = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"wide"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"right"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"c0"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"c1"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"c2"}]}]}]}]}]}"""
        const val ATOM_DOCUMENT = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"a"},{"type":"mention","attrs":{"id":"m1","label":"Sam","metadata":{"kind":"person"}}}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"b"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"c"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"d"}]}]}]}]}]}"""
    }
}
