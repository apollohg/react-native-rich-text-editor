package com.apollohg.editor

import android.app.Activity
import android.graphics.Rect
import android.os.Looper
import android.text.InputType
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.inputmethod.EditorInfo
import android.widget.FrameLayout
import com.apollohg.editor.tables.TableToolbarTestItems
import com.apollohg.editor.tables.activeTableCellPosition
import com.apollohg.editor.tables.pressKeyboardToolbarButton
import com.apollohg.editor.tables.required
import com.apollohg.editor.tables.selectTableCells
import com.apollohg.editor.tables.tableCellPositions
import com.apollohg.editor.viewer.PreparedProseDrawingView
import java.time.Duration
import org.json.JSONObject
import org.json.JSONArray
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class NativeEditorExpoViewTableCellTest : NativeEditorExpoViewTestSupport() {
    private val config = """{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"""
    private val document = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"First"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Second"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"""
    private val strongMarkConfig = config.replace("\"marks\":[]",
        "\"marks\":[{\"name\":\"${TableToolbarTestItems.STRONG_MARK}\"}]")
    private val wideDocument = document.replace("\"type\":\"table_cell\",\"content\"",
        "\"type\":\"table_cell\",\"attrs\":{\"colwidth\":[600]},\"content\"")

    private fun twoRowDocument(): String = JSONObject(document).also { doc ->
        doc.getJSONArray("content").getJSONObject(0).getJSONArray("content").put(JSONObject(
            """{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Third"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Fourth"}]}]}]}"""
        ))
    }.toString()

    private fun rowspanDocument(): String = JSONObject(document).also { doc ->
        val rows = doc.getJSONArray("content").getJSONObject(0).getJSONArray("content")
        rows.getJSONObject(0).getJSONArray("content").getJSONObject(0)
            .put("attrs", JSONObject().put("rowspan", 2))
        rows.put(JSONObject("""{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Third"}]}]}]}"""))
    }.toString()

    private fun cellTexts(adapter: EditorV2Adapter): List<String> {
        val cells = JSONObject(requireNotNull(adapter.documentJson())).getJSONArray("content")
            .getJSONObject(0).getJSONArray("content").getJSONObject(0).getJSONArray("content")
        return (0 until cells.length()).map { index ->
            cells.getJSONObject(index).getJSONArray("content").getJSONObject(0)
                .getJSONArray("content").getJSONObject(0).getString("text")
        }
    }

    private fun proseText(adapter: EditorV2Adapter): String =
        JSONObject(requireNotNull(adapter.documentJson())).getJSONArray("content")
            .getJSONObject(1).getJSONArray("content").getJSONObject(0).getString("text")

    private fun pressTab(input: EditorEditText, shift: Boolean = false,
                         downTime: Long = 100L): Boolean =
        input.dispatchKeyEvent(KeyEvent(downTime, downTime, KeyEvent.ACTION_DOWN,
            KeyEvent.KEYCODE_TAB, 0, if (shift) KeyEvent.META_SHIFT_ON else 0))

    private fun pressArrow(input: EditorEditText, keyCode: Int, downTime: Long = 200L): Boolean =
        input.dispatchKeyEvent(KeyEvent(downTime, downTime, KeyEvent.ACTION_DOWN, keyCode, 0))

    private fun tableRows(adapter: EditorV2Adapter) =
        JSONObject(requireNotNull(adapter.documentJson())).getJSONArray("content")
            .getJSONObject(0).getJSONArray("content")

    private fun tapProse(view: NativeEditorExpoView) {
        val root = view.richTextView.editorEditText
        val offset = root.text.toString().indexOf("after")
        require(offset >= 0)
        val line = root.layout.getLineForOffset(offset)
        val rootOrigin = Rect(0, 0, 1, 1)
        view.richTextView.offsetDescendantRectToMyCoords(root, rootOrigin)
        val x = rootOrigin.left + root.totalPaddingLeft + 8f
        val y = rootOrigin.top + root.totalPaddingTop + root.layout.getLineTop(line) + 8f
        val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
        val up = MotionEvent.obtain(0, 10, MotionEvent.ACTION_UP, x, y, 0)
        try {
            assertTrue(view.richTextView.dispatchTouchEvent(down))
            assertTrue(view.richTextView.dispatchTouchEvent(up))
        } finally {
            down.recycle()
            up.recycle()
        }
    }

    private fun withActiveCell(
        initialDocument: String = document,
        editorConfig: String = config,
        direction: Int = View.LAYOUT_DIRECTION_LTR,
        block: (NativeEditorExpoView, EditorEditText, EditorV2Adapter) -> Unit
    ) {
        val activity = Robolectric.buildActivity(Activity::class.java).setup().get()
        val created = UniffiEditorV2Backend.create(editorConfig, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(
            UniffiEditorV2Backend, JSONObject(created.value).getString("editorId"), false))
        val update = requireNotNull(adapter.setContentJson(initialDocument)) {
            adapter.debugNotes.toString()
        }
        val token = EditorV2Registry.register(adapter)
        try {
            val expo = testExpoContext(activity)
            val view = NativeEditorExpoView(expo.context, expo.appContext)
            view.layoutDirection = direction
            view.richTextView.layoutDirection = direction
            view.richTextView.editorEditText.layoutDirection = direction
            view.onFocusChangeForTesting = {}
            view.onAddonEventForTesting = {}
            view.onEditorReadyForTesting = {}
            view.onEditorUpdateForTesting = {}
            view.onSelectionChangeForTesting = {}
            view.onContentHeightChangeForTesting = {}
            view.onAtomLayoutForTesting = {}
            view.onTableSelectionGeometryForTesting = {}
            val host = FrameLayout(activity)
            activity.setContentView(host)
            host.addView(view, FrameLayout.LayoutParams(600, 500))
            view.setAttachedToNativeWindowForTesting(true)
            view.setEditorId(token)
            assertTrue(view.richTextView.editorEditText.applyUpdateJSON(update))
            view.measure(View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY))
            view.layout(0, 0, 600, 500)
            view.richTextView.measure(View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY))
            view.richTextView.layout(0, 0, 600, 500)
            tapCell(view, 0)
            val input = view.richTextView.activeTextInput
            assertTrue(input !== view.richTextView.editorEditText)
            block(view, input, adapter)
        } finally {
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `reset snapshot with reused table source identity clears mounted scroll`() =
        withActiveCell(initialDocument = wideDocument) { view, _, adapter ->
            val canvas = (0 until view.richTextView.editorContentFrame.childCount)
                .map { view.richTextView.editorContentFrame.getChildAt(it) }
                .filterIsInstance<PreparedProseDrawingView>().single()
            val initial = requireNotNull(canvas.preparedLayout?.blocks?.single()?.tableSurface)
            val sourceId = adapter.cachedTableRecords.values.single().getString("sourceId")
            canvas.setTableLogicalOffset(initial.identity, 150f)
            assertEquals(150f, canvas.tablePhysicalOffsetForTesting(initial.identity), 0.01f)

            val snapshot = requireNotNull(adapter.refreshFromRustState(null))
            assertEquals(PendingEditorUpdateApplyOutcome.APPLIED,
                view.applyEditorResetUpdateOutcome(snapshot))
            view.richTextView.measure(View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY))
            view.richTextView.layout(0, 0, 600, 500)

            val replacement = requireNotNull(canvas.preparedLayout?.blocks?.single()?.tableSurface)
            assertEquals(sourceId, adapter.cachedTableRecords.values.single().getString("sourceId"))
            assertEquals(0f, canvas.tablePhysicalOffsetForTesting(replacement.identity), 0.01f)
        }

    @Test
    fun `public focus keeps the active cell as input target`() = withActiveCell { view, input, _ ->
        input.clearFocus()
        view.focus()
        assertSame(input, view.richTextView.activeTextInput)
        assertTrue(input.hasFocus())
        assertTrue(view.isEditorEffectivelyFocusedForNativeAction())
    }

    @Test
    fun `focused cell input keeps the keyboard toolbar attached`() = withActiveCell { view, input, _ ->
        shadowOf(Looper.getMainLooper()).idle()
        assertTrue(input.hasFocus())
        assertFalse(view.richTextView.editorEditText.hasFocus())
        val content = view.rootView.findViewById<View>(android.R.id.content)
        assertSame("the keyboard toolbar stays attached while a cell has focus", content,
            view.keyboardToolbarView.parent)
        assertNotEquals("the keyboard toolbar is not dismissed while a cell has focus", View.GONE,
            view.keyboardToolbarView.visibility)
    }

    private fun showKeyboard(view: NativeEditorExpoView) {
        view.setCurrentImeBottomForTesting(KEYBOARD_HEIGHT_PX)
        view.updateAttachedKeyboardToolbarForInsetsForTesting()
    }

    private fun assertSharesRootKeyboardClearance(view: NativeEditorExpoView, input: EditorEditText) {
        val root = view.richTextView.editorEditText
        assertTrue("the root reserves keyboard room: ${root.viewportBottomInsetPx}", root.viewportBottomInsetPx > 0)
        assertEquals("the focused cell reserves the root's keyboard room",
            root.viewportBottomInsetPx, input.viewportBottomInsetPx)
        assertEquals("the focused cell sees the keyboard toolbar's top",
            root.viewportBottomOcclusionTopOnScreenPx, input.viewportBottomOcclusionTopOnScreenPx)
    }

    @Test
    fun `keyboard shown over a focused cell gives its caret the root clearance`() = withActiveCell { view, input, _ ->
        showKeyboard(view)
        assertSharesRootKeyboardClearance(view, input)
        tapCell(view, 1)
        assertSame(input, view.richTextView.activeTextInput)
        assertSharesRootKeyboardClearance(view, input)
    }

    @Test
    fun `cell tapped while the keyboard is up inherits the root clearance`() = withActiveCell { view, input, _ ->
        view.richTextView.editorTableSurface.invalidateCell()
        assertTrue(view.richTextView.editorEditText.requestFocus())
        showKeyboard(view)
        assertEquals("a released cell does not follow the keyboard", 0, input.viewportBottomInsetPx)
        tapCell(view, 1)
        assertSame(input, view.richTextView.activeTextInput)
        assertSharesRootKeyboardClearance(view, input)
    }

    @Test
    fun `keyboard toolbar mark press toggles the mark in the focused cell`() =
        withActiveCell(editorConfig = strongMarkConfig) { view, input, adapter ->
            view.setToolbarItemsJson(TableToolbarTestItems.STRONG_JSON)
            input.setSelection(0, input.text.length)
            view.pressKeyboardToolbarButton(TableToolbarTestItems.STRONG_LABEL)

            val runs = { cell: Int ->
                tableRows(adapter).getJSONObject(0).getJSONArray("content").getJSONObject(cell)
                    .getJSONArray("content").getJSONObject(0).getJSONArray("content")
            }
            val focused = runs(0).getJSONObject(0)
            assertEquals("$focused", "First", focused.getString("text"))
            assertEquals("the toolbar marks the focused cell's text: $focused", TableToolbarTestItems.STRONG_MARK,
                focused.optJSONArray("marks")?.getJSONObject(0)?.getString("type"))
            assertFalse("the neighbouring cell is untouched: ${runs(1)}", runs(1).getJSONObject(0).has("marks"))
        }

    private fun authoritativeSelection(adapter: EditorV2Adapter): String =
        JSONObject(requireNotNull(adapter.cachedAtomicRenderJson)).getJSONObject("selection").toString()

    private fun pressAfterPendingUpdates(view: NativeEditorExpoView, label: String) {
        shadowOf(Looper.getMainLooper())
            .idleFor(Duration.ofMillis(NativeEditorExpoView.EDITOR_UPDATE_EVENT_DEBOUNCE_MS))
        view.pressKeyboardToolbarButton(label)
    }

    private fun typeAtCellEnd(input: EditorEditText, text: String) {
        input.setSelection(input.text.length)
        assertTrue(requireNotNull(input.onCreateInputConnection(EditorInfo())).commitText(text, 1))
    }

    @Test
    fun `keyboard toolbar undo and redo apply under a cell rectangle`() =
        withActiveCell { view, input, adapter ->
            view.setToolbarItemsJson(TableToolbarTestItems.HISTORY_JSON)
            val root = view.richTextView.editorEditText
            val originalSelection = authoritativeSelection(adapter)
            typeAtCellEnd(input, "X")
            val editedSelection = authoritativeSelection(adapter)
            assertEquals("typing edits the cell", listOf("FirstX", "Second"), cellTexts(adapter))
            val positions = adapter.tableCellPositions(adapter.cachedTableRecords.keys.single())
            root.selectTableCells(adapter, positions.first(), positions.last())
            assertTrue("the rectangle is authoritative on the root", root.authoritativeCellSelectionActive)
            assertSame("the rectangle retires the cell input", root, view.richTextView.activeTextInput)
            assertTrue(root.hasFocus())
            val focus = recordFocus(view)

            pressAfterPendingUpdates(view, TableToolbarTestItems.UNDO_LABEL)
            assertEquals("undo under a rectangle restores the document", listOf("First", "Second"), cellTexts(adapter))
            assertEquals("undo resolves to the selection before the edit",
                originalSelection, authoritativeSelection(adapter))
            assertFalse("undo leaves no stale rectangle on the root", root.authoritativeCellSelectionActive)
            assertSame("the cell holding the restored caret takes the input", input, view.richTextView.activeTextInput)
            assertEquals(positions[0].toLong(), view.richTextView.activeTableCellPosition)
            assertTrue("the bound cell takes focus", input.hasFocus())
            assertFalse(root.hasFocus())
            assertEquals("First", input.text.toString())
            assertEquals("the caret is restored in the cell", "First".length, input.selectionStart)

            pressAfterPendingUpdates(view, TableToolbarTestItems.REDO_LABEL)
            assertEquals("redo reapplies the edit", listOf("FirstX", "Second"), cellTexts(adapter))
            assertEquals("redo resolves to the selection after the edit",
                editedSelection, authoritativeSelection(adapter))
            assertEquals(positions[0].toLong(), view.richTextView.activeTableCellPosition)
            assertTrue(input.hasFocus())
            assertEquals("FirstX".length, input.selectionStart)
            shadowOf(Looper.getMainLooper()).idle()
            assertFalse("the rectangle-to-cell handoff never blurs: $focus", focus.contains(false))
            assertEquals("the cell handoff does not restart the retiring root connection", 0,
                root.imeTraceSnapshotForTesting().count { it.startsWith(CELL_SELECTION_EXIT_RESTART) })

            view.setEditable(false)
            view.richTextView.activeTextInput.performToolbarUndo()
            assertEquals("a read-only editor refuses undo", listOf("FirstX", "Second"), cellTexts(adapter))
        }

    @Test
    fun `keyboard toolbar undo and redo apply through the bound cell`() =
        withActiveCell { view, input, adapter ->
            view.setToolbarItemsJson(TableToolbarTestItems.HISTORY_JSON)
            val originalSelection = authoritativeSelection(adapter)
            typeAtCellEnd(input, "X")
            val editedSelection = authoritativeSelection(adapter)
            assertEquals("typing edits the cell", listOf("FirstX", "Second"), cellTexts(adapter))

            val authority = input.tableCellInputAuthority
            input.tableCellInputAuthority = { false }
            input.performToolbarUndo()
            assertEquals("a cell without binding authority refuses undo", listOf("FirstX", "Second"), cellTexts(adapter))
            input.tableCellInputAuthority = authority

            pressAfterPendingUpdates(view, TableToolbarTestItems.UNDO_LABEL)
            assertEquals("undo through the bound cell restores the document", listOf("First", "Second"), cellTexts(adapter))
            assertEquals("undo resolves to the selection before the edit",
                originalSelection, authoritativeSelection(adapter))
            assertSame("the restored caret stays in the bound cell", input, view.richTextView.activeTextInput)
            assertEquals("the bound cell shows the restored caret", input.text.length, input.selectionStart)

            pressAfterPendingUpdates(view, TableToolbarTestItems.REDO_LABEL)
            assertEquals("redo reapplies the edit", listOf("FirstX", "Second"), cellTexts(adapter))
            assertEquals("redo resolves to the selection after the edit",
                editedSelection, authoritativeSelection(adapter))
            assertSame("the reapplied caret stays in the bound cell", input, view.richTextView.activeTextInput)
            assertEquals("the bound cell shows the reapplied caret", input.text.length, input.selectionStart)
        }

    @Test
    fun `keyboard toolbar undo settles the bound cell composition first`() =
        withActiveCell { view, input, adapter ->
            view.setToolbarItemsJson(TableToolbarTestItems.HISTORY_JSON)
            typeAtCellEnd(input, "X")
            val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(connection.setComposingText("zz", 1))
            assertTrue("the cell is composing", input.hasPendingCompositionForExternalRefresh())

            pressAfterPendingUpdates(view, TableToolbarTestItems.UNDO_LABEL)
            assertFalse("undo commits the cell composition first", input.hasPendingCompositionForExternalRefresh())
            assertEquals("undo reverts the committed composition as its own step",
                listOf("FirstX", "Second"), cellTexts(adapter))

            pressAfterPendingUpdates(view, TableToolbarTestItems.REDO_LABEL)
            assertEquals("redo restores the committed composition exactly once",
                listOf("FirstXzz", "Second"), cellTexts(adapter))
        }

    @Test
    fun `keyboard toolbar mark press settles the bound cell composition first`() =
        withActiveCell(editorConfig = strongMarkConfig) { view, input, adapter ->
            view.setToolbarItemsJson(TableToolbarTestItems.STRONG_JSON)
            input.setSelection(input.text.length)
            val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(connection.setComposingText("zz", 1))
            assertTrue("the cell is composing", input.hasPendingCompositionForExternalRefresh())

            pressAfterPendingUpdates(view, TableToolbarTestItems.STRONG_LABEL)
            assertFalse("the mark press commits the cell composition first", input.hasPendingCompositionForExternalRefresh())
            assertEquals("the composition lands in the focused cell", listOf("Firstzz", "Second"), cellTexts(adapter))
        }

    private fun recordFocus(view: NativeEditorExpoView): MutableList<Any?> {
        val changes = mutableListOf<Any?>()
        view.onFocusChangeForTesting = { changes += it["isFocused"] }
        return changes
    }

    private fun typeInProse(view: NativeEditorExpoView, text: String): EditorEditText {
        tapProse(view)
        val root = view.richTextView.editorEditText
        assertSame(root, view.richTextView.activeTextInput)
        assertTrue(requireNotNull(root.onCreateInputConnection(EditorInfo())).commitText(text, 1))
        return root
    }

    @Test
    fun `keyboard toolbar undo from a bound cell rebinds the cell holding the restored caret`() =
        withActiveCell { view, input, adapter ->
            view.setToolbarItemsJson(TableToolbarTestItems.HISTORY_JSON)
            typeAtCellEnd(input, "X")
            val editedSelection = authoritativeSelection(adapter)
            val positions = adapter.tableCellPositions(adapter.cachedTableRecords.keys.single())
            tapCell(view, 1)
            assertEquals(positions[1].toLong(), view.richTextView.activeTableCellPosition)
            val focus = recordFocus(view)

            pressAfterPendingUpdates(view, TableToolbarTestItems.UNDO_LABEL)
            assertEquals(listOf("First", "Second"), cellTexts(adapter))
            assertSame("the restored caret keeps the cell input", input, view.richTextView.activeTextInput)
            assertEquals("the cell holding the restored caret is bound",
                positions[0].toLong(), view.richTextView.activeTableCellPosition)
            assertTrue("the rebound cell keeps focus", input.hasFocus())
            assertEquals("First", input.text.toString())
            assertEquals("the caret is restored in the rebound cell", "First".length, input.selectionStart)

            pressAfterPendingUpdates(view, TableToolbarTestItems.REDO_LABEL)
            assertEquals(listOf("FirstX", "Second"), cellTexts(adapter))
            assertEquals(editedSelection, authoritativeSelection(adapter))
            assertEquals(positions[0].toLong(), view.richTextView.activeTableCellPosition)
            assertTrue(input.hasFocus())
            assertEquals("the caret follows the reapplied edit", "FirstX".length, input.selectionStart)
            shadowOf(Looper.getMainLooper()).idle()
            assertFalse("moving between cells never blurs: $focus", focus.contains(false))
        }

    @Test
    fun `javascript undo under a cell rectangle binds the cell holding the restored caret`() =
        withActiveCell { view, input, adapter ->
            typeAtCellEnd(input, "X")
            val positions = adapter.tableCellPositions(adapter.cachedTableRecords.keys.single())
            val root = view.richTextView.editorEditText
            root.selectTableCells(adapter, positions.first(), positions.last())
            assertSame("the rectangle retires the cell input", root, view.richTextView.activeTextInput)
            assertTrue(root.hasFocus())

            adapter.callWithEnvelope(JSONObject(), includeBaseRevision = false) {
                UniffiEditorV2Backend.undo(adapter.editorId, it)
            }
                .required("javascript undo")
            assertTrue(view.applyEditorUpdate(
                UniffiEditorV2Backend.renderUpdate(adapter.editorId, null, null).required("render update")))

            assertEquals("the JavaScript undo restores the document", listOf("First", "Second"), cellTexts(adapter))
            assertSame("the cell holding the restored caret takes the input", input, view.richTextView.activeTextInput)
            assertEquals(positions[0].toLong(), view.richTextView.activeTableCellPosition)
            assertTrue("the focused editor keeps focus in the bound cell", input.hasFocus())
            assertEquals("the caret is restored in the cell", "First".length, input.selectionStart)
        }

    @Test
    fun `undo rebinding an unfocused cell leaves focus where it was`() =
        withActiveCell { view, input, adapter ->
            typeAtCellEnd(input, "X")
            val positions = adapter.tableCellPositions(adapter.cachedTableRecords.keys.single())
            tapCell(view, 1)
            input.clearFocus()
            assertFalse(input.hasFocus())
            val root = view.richTextView.editorEditText
            val rootFocusedBeforeUndo = root.hasFocus()

            input.performToolbarUndo()
            assertEquals(listOf("First", "Second"), cellTexts(adapter))
            assertEquals("the cell holding the restored caret is bound",
                positions[0].toLong(), view.richTextView.activeTableCellPosition)
            assertEquals("First".length, input.selectionStart)
            assertFalse("rebinding an unfocused cell does not grab focus", input.hasFocus())
            assertEquals("the root keeps its focus state (focused before undo: $rootFocusedBeforeUndo)",
                rootFocusedBeforeUndo, root.hasFocus())
        }

    @Test
    fun `external update while a cell composes applies the settled commit once`() =
        withActiveCell { view, input, adapter ->
            val external = requireNotNull(adapter.cachedAtomicRenderJson)
            val revision = adapter.baseDocumentRevision
            input.setSelection(input.text.length)
            assertTrue(requireNotNull(input.onCreateInputConnection(EditorInfo())).setComposingText("zz", 1))

            assertTrue(view.applyEditorUpdate(external))
            assertEquals("the composition lands once", listOf("Firstzz", "Second"), cellTexts(adapter))
            assertEquals("the settle is the only new revision", revision + 1uL, adapter.baseDocumentRevision)
            val root = view.richTextView.editorEditText
            assertEquals(adapter.baseDocumentRevision.toString(), root.lastAppliedDocumentVersion)
            assertSame("the cell stays bound", input, view.richTextView.activeTextInput)
            assertEquals("Firstzz", input.text.toString())
            assertEquals("Firstzz".length, input.selectionStart)
            assertFalse(input.hasPendingCompositionForExternalRefresh())
        }

    @Test
    fun `keyboard toolbar undo from a bound cell restoring a prose caret hands focus to the root`() =
        withActiveCell { view, input, adapter ->
            view.setToolbarItemsJson(TableToolbarTestItems.HISTORY_JSON)
            val root = typeInProse(view, "Y")
            assertEquals("afterY", proseText(adapter).take("afterY".length))
            val proseCaret = root.selectionStart
            tapCell(view, 0)
            assertSame(input, view.richTextView.activeTextInput)
            val focus = recordFocus(view)

            pressAfterPendingUpdates(view, TableToolbarTestItems.UNDO_LABEL)
            assertEquals("after", proseText(adapter))
            assertSame("a prose caret retires the cell input", root, view.richTextView.activeTextInput)
            assertTrue("the root takes over focus", root.hasFocus())
            assertFalse(input.hasFocus())
            assertEquals("the root shows the restored caret", proseCaret - 1, root.selectionStart)
            assertFalse("the restored prose caret accepts input", root.rootTableSelectionInputBlocked)
            shadowOf(Looper.getMainLooper()).idle()
            assertFalse("the cell-to-root handoff never blurs: $focus", focus.contains(false))
        }

    @Test
    fun `keyboard toolbar undo leaving a rectangle for prose restarts the root input connection`() =
        withActiveCell { view, _, adapter ->
            view.setToolbarItemsJson(TableToolbarTestItems.HISTORY_JSON)
            val root = typeInProse(view, "Y")
            val proseCaret = root.selectionStart
            val positions = adapter.tableCellPositions(adapter.cachedTableRecords.keys.single())
            root.selectTableCells(adapter, positions.first(), positions.last())
            assertTrue(root.authoritativeCellSelectionActive)
            val restartsBefore = root.imeTraceSnapshotForTesting().count { it.startsWith(CELL_SELECTION_EXIT_RESTART) }

            pressAfterPendingUpdates(view, TableToolbarTestItems.UNDO_LABEL)
            assertEquals("after", proseText(adapter))
            assertFalse(root.authoritativeCellSelectionActive)
            assertTrue(root.hasFocus())
            assertEquals(proseCaret - 1, root.selectionStart)
            assertEquals("the retired root connection restarts for the prose caret", restartsBefore + 1,
                root.imeTraceSnapshotForTesting().count { it.startsWith(CELL_SELECTION_EXIT_RESTART) })
        }

    @Test
    fun `keyboard toolbar undo is refused and retried while the cell cannot drain`() =
        withActiveCell { view, input, adapter ->
            view.setToolbarItemsJson(TableToolbarTestItems.HISTORY_JSON)
            typeAtCellEnd(input, "X")
            input.blockExternalEditorUpdatePreparationForTesting = true
            pressAfterPendingUpdates(view, TableToolbarTestItems.UNDO_LABEL)
            assertEquals("an undrained cell refuses undo", listOf("FirstX", "Second"), cellTexts(adapter))
            assertTrue("the refused press is retried", view.hasPendingNativeActionForTesting())
            input.blockExternalEditorUpdatePreparationForTesting = false
            shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(NativeEditorExpoView.NATIVE_ACTION_RETRY_DELAY_MS))
            assertEquals("the retry applies undo once the cell drains", listOf("First", "Second"), cellTexts(adapter))
        }

    @Test
    fun `keyboard toolbar action settling a composing cell carries the settled update`() =
        withActiveCell { view, input, adapter ->
            view.setToolbarItemsJson(ACTION_TOOLBAR_JSON)
            val payloads = mutableListOf<Map<String, Any>>()
            view.onToolbarActionForTesting = { payloads += it }
            input.setSelection(input.text.length)
            assertTrue(requireNotNull(input.onCreateInputConnection(EditorInfo())).setComposingText("zz", 1))

            pressAfterPendingUpdates(view, ACTION_LABEL)
            assertEquals(listOf("Firstzz", "Second"), cellTexts(adapter))
            val payload = payloads.single()
            assertEquals(ACTION_KEY, payload["key"])
            assertEquals("the action reports the settled revision: $payload",
                adapter.baseDocumentRevision.toString(), payload["documentRevision"]?.toString())
            assertTrue("the action carries the settled update: $payload",
                (payload["updateJson"] as? String)?.contains("Firstzz") == true)
        }

    @Test
    fun `hardware Tab moves into the next cell on the reusable input`() =
        withActiveCell { view, input, adapter ->
            assertTrue(pressTab(input))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Second", input.text.toString())
            assertEquals(listOf("First", "Second"), cellTexts(adapter))
        }

    @Test
    fun `right arrow at LTR cell end enters the physical neighbor without changing document`() =
        withActiveCell { view, input, adapter ->
            input.setSelection(input.text.length)
            val before = adapter.documentJson()
            val revision = adapter.baseDocumentRevision
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_RIGHT))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Second", input.text.toString())
            assertEquals(before, adapter.documentJson())
            assertEquals(revision, adapter.baseDocumentRevision)
        }

    @Test
    fun `horizontal arrows wrap rows without appending a row`() =
        withActiveCell(initialDocument = twoRowDocument()) { view, input, adapter ->
            tapCell(view, 1)
            input.setSelection(input.text.length)
            val revision = adapter.baseDocumentRevision
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_RIGHT, 201L))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Third", input.text.toString())
            input.setSelection(0)
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_LEFT, 202L))
            assertEquals("Second", input.text.toString())
            assertEquals(2, tableRows(adapter).length())
            assertEquals(revision, adapter.baseDocumentRevision)
        }

    @Test
    fun `vertical arrows cross the adjacent row without changing document`() =
        withActiveCell(initialDocument = twoRowDocument()) { view, input, adapter ->
            input.setSelection(input.text.length)
            val revision = adapter.baseDocumentRevision
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_DOWN, 203L))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Third", input.text.toString())
            input.setSelection(0)
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_UP, 204L))
            assertEquals("First", input.text.toString())
            assertEquals(revision, adapter.baseDocumentRevision)
        }

    @Test
    fun `horizontal row wrap skips a cell spanning the preceding row`() =
        withActiveCell(initialDocument = rowspanDocument()) { view, input, adapter ->
            tapCell(view, 1)
            input.setSelection(input.text.length)
            val revision = adapter.baseDocumentRevision
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_RIGHT, 205L))
            assertEquals("Third", input.text.toString())
            assertEquals(revision, adapter.baseDocumentRevision)
        }

    @Test
    fun `right arrow inside cell keeps native text movement`() =
        withActiveCell { view, input, adapter ->
            input.setSelection(1)
            assertEquals(1, input.selectionStart)
            val nativeDestination = input.layout.getOffsetToRightOf(1)
            val revision = adapter.baseDocumentRevision
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_RIGHT, 206L))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("First", input.text.toString())
            assertEquals(nativeDestination, input.selectionStart)
            assertEquals(revision, adapter.baseDocumentRevision)
        }

    @Test
    fun `right arrow at final LTR cell exits into following prose without adding a row`() =
        withActiveCell { view, input, adapter ->
            tapCell(view, 1)
            input.setSelection(input.text.length)
            val revision = adapter.baseDocumentRevision
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_RIGHT, 207L))
            val root = view.richTextView.editorEditText
            assertSame(root, view.richTextView.activeTextInput)
            assertEquals(root.text.toString().indexOf("after"), root.selectionStart)
            assertEquals(revision, adapter.baseDocumentRevision)
            val connection = requireNotNull(root.onCreateInputConnection(EditorInfo()))
            assertTrue(connection.commitText("X", 1))
            assertEquals("Xafter", proseText(adapter))
            assertEquals(1, tableRows(adapter).length())
        }

    @Test
    fun `left arrow enters the physical neighbor in RTL`() {
        val editorConfig = JSONObject(config)
        val nodes = editorConfig.getJSONObject("schema").getJSONArray("nodes")
        (0 until nodes.length()).map { nodes.getJSONObject(it) }
            .first { it.getString("name") == "table" }
            .put("attrs", JSONObject().put("dir", JSONObject().put("default", "ltr")))
        val rtlDocument = JSONObject(document)
        rtlDocument.getJSONArray("content").getJSONObject(0)
            .put("attrs", JSONObject().put("dir", "rtl"))
        withActiveCell(initialDocument = rtlDocument.toString(),
            editorConfig = editorConfig.toString(), direction = View.LAYOUT_DIRECTION_RTL) {
                view, input, adapter ->
            input.setSelection(0)
            val revision = adapter.baseDocumentRevision
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_LEFT, 208L))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Second", input.text.toString())
            assertEquals(revision, adapter.baseDocumentRevision)
        }
    }

    @Test
    fun `right arrow enters RTL text at its visual left edge and next arrow stays in cell`() {
        val mixed = JSONObject(document)
        mixed.getJSONArray("content").getJSONObject(0).getJSONArray("content")
            .getJSONObject(0).getJSONArray("content").getJSONObject(1)
            .getJSONArray("content").getJSONObject(0).getJSONArray("content")
            .getJSONObject(0).put("text", "אבג")
        withActiveCell(initialDocument = mixed.toString()) { view, input, adapter ->
            input.setSelection(input.text.length)
            val revision = adapter.baseDocumentRevision
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_RIGHT, 221L))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("אבג", input.text.toString())
            val entry = input.selectionStart
            assertEquals(entry, input.layout.getOffsetToLeftOf(entry))
            val nativeNext = input.layout.getOffsetToRightOf(entry)
            assertTrue(nativeNext != entry)
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_RIGHT, 222L))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("אבג", input.text.toString())
            assertEquals(nativeNext, input.selectionStart)
            assertEquals(revision, adapter.baseDocumentRevision)
        }
    }

    @Test
    fun `left arrow in RTL table enters LTR text at its visual right edge`() {
        val editorConfig = JSONObject(config)
        val nodes = editorConfig.getJSONObject("schema").getJSONArray("nodes")
        (0 until nodes.length()).map { nodes.getJSONObject(it) }
            .first { it.getString("name") == "table" }
            .put("attrs", JSONObject().put("dir", JSONObject().put("default", "ltr")))
        val rtlDocument = JSONObject(document)
        rtlDocument.getJSONArray("content").getJSONObject(0)
            .put("attrs", JSONObject().put("dir", "rtl"))
        withActiveCell(initialDocument = rtlDocument.toString(),
            editorConfig = editorConfig.toString(), direction = View.LAYOUT_DIRECTION_RTL) {
                view, input, adapter ->
            input.setSelection(0)
            val revision = adapter.baseDocumentRevision
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_LEFT, 223L))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Second", input.text.toString())
            val entry = input.selectionStart
            assertEquals(entry, input.layout.getOffsetToRightOf(entry))
            val nativeNext = input.layout.getOffsetToLeftOf(entry)
            assertTrue(nativeNext != entry)
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_LEFT, 224L))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Second", input.text.toString())
            assertEquals(nativeNext, input.selectionStart)
            assertEquals(revision, adapter.baseDocumentRevision)
        }
    }

    @Test
    fun `left arrow at first cell exits into preceding prose`() {
        val withBefore = JSONObject(document)
        val original = withBefore.getJSONArray("content")
        val content = JSONArray().put(JSONObject(
            """{"type":"paragraph","content":[{"type":"text","text":"before"}]}"""))
        for (index in 0 until original.length()) content.put(original.get(index))
        withBefore.put("content", content)
        withActiveCell(initialDocument = withBefore.toString()) { view, input, adapter ->
            input.setSelection(0)
            val revision = adapter.baseDocumentRevision
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_LEFT, 209L))
            val root = view.richTextView.editorEditText
            assertSame(root, view.richTextView.activeTextInput)
            assertEquals(root.text.toString().indexOf("before") + "before".length,
                root.selectionStart)
            assertEquals(revision, adapter.baseDocumentRevision)
        }
    }

    @Test
    fun `down arrow at final row exits into following prose`() =
        withActiveCell { view, input, adapter ->
            val revision = adapter.baseDocumentRevision
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_DOWN, 210L))
            val root = view.richTextView.editorEditText
            assertSame(root, view.richTextView.activeTextInput)
            assertEquals(root.text.toString().indexOf("after"), root.selectionStart)
            assertEquals(revision, adapter.baseDocumentRevision)
        }

    @Test
    fun `input connection arrow navigates and retires its old connection`() =
        withActiveCell { view, input, adapter ->
            input.setSelection(input.text.length)
            val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            val revision = adapter.baseDocumentRevision
            assertTrue(connection.sendKeyEvent(KeyEvent(211L, 211L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_RIGHT, 0)))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Second", input.text.toString())
            assertFalse(connection.beginBatchEdit())
            val destination = input.selectionStart
            connection.sendKeyEvent(KeyEvent(211L, 211L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_RIGHT, 0))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Second", input.text.toString())
            assertEquals(destination, input.selectionStart)
            assertEquals(revision, adapter.baseDocumentRevision)
        }

    @Test
    fun `stale table epoch consumes arrow without moving or writing`() =
        withActiveCell { view, input, adapter ->
            input.setSelection(input.text.length)
            val revision = adapter.baseDocumentRevision
            adapter.positionEpoch = "999"
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_RIGHT, 212L))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("First", input.text.toString())
            assertEquals(revision, adapter.baseDocumentRevision)
        }

    @Test
    fun `arrow commits composition once before entering next cell`() =
        withActiveCell { view, input, adapter ->
            val oldConnection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            input.setSelection(input.text.length)
            assertTrue(oldConnection.setComposingText("X", 1))
            val revision = adapter.baseDocumentRevision
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_RIGHT, 214L))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Second", input.text.toString())
            assertFalse(oldConnection.beginBatchEdit())
            assertEquals(revision + 1uL, adapter.baseDocumentRevision)
            assertEquals(listOf("FirstX", "Second"), cellTexts(adapter))
        }

    @Test
    fun `owner rebind does not revive the retired arrow input connection`() =
        withActiveCell { view, input, adapter ->
            input.setSelection(input.text.length)
            val oldConnection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            val token = input.editorId
            val revision = adapter.baseDocumentRevision
            view.setEditorId(0L)
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_RIGHT, 215L))
            assertEquals(revision, adapter.baseDocumentRevision)
            assertFalse(oldConnection.beginBatchEdit())
            view.setEditorId(token)
            tapCell(view, 0)
            input.setSelection(input.text.length)
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_RIGHT, 216L))
            assertEquals("Second", input.text.toString())
            oldConnection.commitText("stale", 1)
            assertEquals(revision, adapter.baseDocumentRevision)
            assertEquals(listOf("First", "Second"), cellTexts(adapter))
        }

    @Test
    fun `duplicate arrow delivery crosses only one cell`() =
        withActiveCell(initialDocument = twoRowDocument()) { view, input, adapter ->
            input.setSelection(input.text.length)
            val arrow = KeyEvent(217L, 217L, KeyEvent.ACTION_DOWN,
                KeyEvent.KEYCODE_DPAD_RIGHT, 0)
            assertTrue(input.dispatchKeyEvent(arrow))
            val destination = input.selectionStart
            assertTrue(input.dispatchKeyEvent(arrow))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Second", input.text.toString())
            assertEquals(destination, input.selectionStart)
            assertEquals(2, tableRows(adapter).length())
        }

    @Test
    fun `hardware repeat count remains a distinct native arrow movement`() =
        withActiveCell { view, input, adapter ->
            input.setSelection(input.text.length)
            val revision = adapter.baseDocumentRevision
            assertTrue(input.dispatchKeyEvent(KeyEvent(225L, 225L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_RIGHT, 0)))
            assertEquals("Second", input.text.toString())
            val entry = input.selectionStart
            val nativeNext = input.layout.getOffsetToRightOf(entry)
            assertTrue(nativeNext != entry)
            assertTrue(input.dispatchKeyEvent(KeyEvent(225L, 240L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_RIGHT, 1)))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals(nativeNext, input.selectionStart)
            assertEquals(revision, adapter.baseDocumentRevision)
        }

    @Test
    fun `modified arrow keeps native selection inside the active cell`() =
        withActiveCell { view, input, adapter ->
            input.setSelection(input.text.length)
            val revision = adapter.baseDocumentRevision
            assertTrue(input.dispatchKeyEvent(KeyEvent(218L, 218L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_LEFT, 0,
                KeyEvent.META_SHIFT_ON)))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("First", input.text.toString())
            assertEquals(revision, adapter.baseDocumentRevision)
        }

    @Test
    fun `control arrow does not switch cells`() =
        withActiveCell { view, input, adapter ->
            input.setSelection(input.text.length)
            val revision = adapter.baseDocumentRevision
            input.dispatchKeyEvent(KeyEvent(219L, 219L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_RIGHT, 0,
                KeyEvent.META_CTRL_ON))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("First", input.text.toString())
            assertEquals(revision, adapter.baseDocumentRevision)
        }

    @Test
    fun `read only transition consumes arrow on retired cell input`() =
        withActiveCell { view, input, adapter ->
            input.setSelection(input.text.length)
            val revision = adapter.baseDocumentRevision
            view.setEditable(false)
            assertTrue(pressArrow(input, KeyEvent.KEYCODE_DPAD_RIGHT, 220L))
            assertSame(view.richTextView.editorEditText, view.richTextView.activeTextInput)
            assertEquals(revision, adapter.baseDocumentRevision)
        }

    @Test
    fun `hardware Shift Tab moves backward to the exact first cell`() =
        withActiveCell { view, input, adapter ->
            tapCell(view, 1)
            assertTrue(pressTab(input, shift = true))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("First", input.text.toString())
            val scalar = JSONObject(requireNotNull(adapter.selectionJson())).getInt("anchorScalar")
            assertEquals(input.currentScalarSelection()?.first,
                input.tableCellPositionMap?.localScalarForGlobalScalar(scalar))
            assertEquals(listOf("First", "Second"), cellTexts(adapter))
        }

    @Test
    fun `hardware Tab crosses a row boundary and Shift Tab returns to the previous row`() =
        withActiveCell(initialDocument = twoRowDocument()) { view, input, adapter ->
            tapCell(view, 1)
            assertEquals("Second", input.text.toString())
            assertTrue(pressTab(input, downTime = 301L))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Third", input.text.toString())
            assertTrue(pressTab(input, shift = true, downTime = 302L))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Second", input.text.toString())
            assertEquals(2, tableRows(adapter).length())
            assertEquals("after", proseText(adapter))
        }

    @Test
    fun `hardware Tab follows document order across physically reversed RTL cells`() {
        val editorConfig = JSONObject(config)
        val nodes = editorConfig.getJSONObject("schema").getJSONArray("nodes")
        (0 until nodes.length()).map { nodes.getJSONObject(it) }
            .first { it.getString("name") == "table" }
            .put("attrs", JSONObject().put("dir", JSONObject().put("default", "ltr")))
        val rtlDocument = JSONObject(document)
        rtlDocument.getJSONArray("content").getJSONObject(0)
            .put("attrs", JSONObject().put("dir", "rtl"))
        withActiveCell(initialDocument = rtlDocument.toString(),
            editorConfig = editorConfig.toString(), direction = View.LAYOUT_DIRECTION_RTL) {
                view, input, adapter ->
            val canvas = (0 until view.richTextView.editorContentFrame.childCount)
                .map { view.richTextView.editorContentFrame.getChildAt(it) }
                .filterIsInstance<PreparedProseDrawingView>().single()
            val surface = requireNotNull(canvas.preparedLayout?.blocks?.singleOrNull()?.tableSurface)
            assertTrue(surface.frameOfCell(0)!!.left > surface.frameOfCell(1)!!.left)
            assertTrue(pressTab(input))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Second", input.text.toString())
            assertEquals(listOf("First", "Second"), cellTexts(adapter))
        }
    }

    @Test
    fun `hardware Shift Tab at the first cell leaves the document unchanged`() =
        withActiveCell { view, input, adapter ->
            val before = adapter.documentJson()
            val revision = adapter.baseDocumentRevision
            assertTrue(pressTab(input, shift = true))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals(before, adapter.documentJson())
            assertEquals(revision, adapter.baseDocumentRevision)
        }

    @Test
    fun `hardware Tab at the last cell appends one row and enters its first cell`() =
        withActiveCell { view, input, adapter ->
            tapCell(view, 1)
            val before = adapter.baseDocumentRevision
            assertTrue(pressTab(input))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals(before + 1uL, adapter.baseDocumentRevision)
            assertEquals("\u200B", input.text.toString())
            val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(input.canDispatchTableCellMutation())
            assertTrue(connection.commitText("N", 1))
            val rows = tableRows(adapter)
            assertEquals(2, rows.length())
            assertEquals(2, rows.getJSONObject(1).getJSONArray("content").length())
            assertEquals("N", rows.getJSONObject(1).getJSONArray("content").getJSONObject(0)
                .getJSONArray("content").getJSONObject(0).getJSONArray("content")
                .getJSONObject(0).getString("text"))
            assertEquals(listOf("First", "Second"), cellTexts(adapter))
            assertEquals("after", proseText(adapter))
        }

    @Test
    fun `hardware Tab commits composition once and retires the old connection`() =
        withActiveCell { view, input, adapter ->
            val oldConnection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            input.setSelection(input.text.length)
            assertTrue(oldConnection.setComposingText("X", 1))
            val before = adapter.baseDocumentRevision
            val tab = KeyEvent(101L, 101L, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_TAB, 0)
            assertTrue(input.dispatchKeyEvent(tab))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Second", input.text.toString())
            assertEquals(listOf("FirstX", "Second"), cellTexts(adapter))
            assertEquals(before + 1uL, adapter.baseDocumentRevision)
            assertFalse(oldConnection.beginBatchEdit())
            oldConnection.commitText("stale", 1)
            oldConnection.sendKeyEvent(tab)
            assertTrue(input.dispatchKeyEvent(tab))
            assertEquals(listOf("FirstX", "Second"), cellTexts(adapter))
            assertEquals(1, tableRows(adapter).length())
        }

    @Test
    fun `input connection hardware Tab follows the same cell navigation route`() =
        withActiveCell { view, input, adapter ->
            val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(connection.sendKeyEvent(KeyEvent(103L, 103L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_TAB, 0)))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Second", input.text.toString())
            assertEquals(listOf("First", "Second"), cellTexts(adapter))
            assertFalse(connection.beginBatchEdit())
        }

    @Test
    fun `stale table binding consumes Tab without changing document`() =
        withActiveCell { _, input, adapter ->
            val before = adapter.documentJson()
            adapter.positionEpoch = "999"
            assertTrue(pressTab(input))
            assertEquals(before, adapter.documentJson())
        }

    @Test
    fun `owner loss before Tab and later rebind do not revive the old connection`() =
        withActiveCell { view, input, adapter ->
            val token = input.editorId
            val oldConnection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            val before = adapter.baseDocumentRevision
            view.setEditorId(0L)
            assertTrue(pressTab(input))
            assertEquals(before, adapter.baseDocumentRevision)
            assertFalse(oldConnection.beginBatchEdit())

            view.setEditorId(token)
            tapCell(view, 0)
            assertSame(input, view.richTextView.activeTextInput)
            assertTrue(pressTab(input, downTime = 501L))
            assertEquals("Second", input.text.toString())
            oldConnection.commitText("stale", 1)
            assertEquals(listOf("First", "Second"), cellTexts(adapter))
            assertEquals(before, adapter.baseDocumentRevision)
        }

    @Test
    fun `owner switch during table Tab cannot write into the new editor`() =
        withActiveCell { view, input, adapter ->
            val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
            val other = requireNotNull(EditorV2Adapter.attach(UniffiEditorV2Backend,
                JSONObject(created.value).getString("editorId"), false))
            requireNotNull(other.setContentJson(document))
            val otherToken = EditorV2Registry.register(other)
            try {
                tapCell(view, 1)
                val oldConnection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
                val otherBefore = other.baseDocumentRevision
                val originalBefore = adapter.baseDocumentRevision
                val root = view.richTextView.editorEditText
                val previous = root.onBeforeRenderRefresh
                var switched = false
                root.onBeforeRenderRefresh = {
                    previous?.invoke()
                    if (!switched) {
                        switched = true
                        view.setEditorId(otherToken)
                    }
                }

                assertTrue(pressTab(input))
                assertTrue(switched)
                assertEquals(otherBefore, other.baseDocumentRevision)
                assertEquals(originalBefore + 1uL, adapter.baseDocumentRevision)
                assertSame(root, view.richTextView.activeTextInput)
                assertFalse(oldConnection.beginBatchEdit())
                oldConnection.commitText("stale", 1)
                assertEquals(otherBefore, other.baseDocumentRevision)
                assertEquals(1, tableRows(other).length())
            } finally {
                view.setEditorId(0L)
                EditorV2Registry.remove(other.editorId)
                other.destroy()
            }
        }

    @Test
    fun `duplicate hardware Tab delivery does not navigate twice`() =
        withActiveCell { view, input, adapter ->
            val tab = KeyEvent(102L, 102L, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_TAB, 0)
            assertTrue(input.dispatchKeyEvent(tab))
            assertTrue(input.dispatchKeyEvent(tab))
            assertSame(input, view.richTextView.activeTextInput)
            assertEquals("Second", input.text.toString())
            assertEquals(1, tableRows(adapter).length())
        }

    @Test
    fun `read only hardware Tab leaves table and active input unchanged`() =
        withActiveCell { view, input, adapter ->
            val before = adapter.documentJson()
            view.setEditable(false)
            assertTrue(pressTab(input))
            assertEquals(before, adapter.documentJson())
            assertSame(view.richTextView.editorEditText, view.richTextView.activeTextInput)
        }

    @Test
    fun `prose Tab does not enter a table or mutate its document`() =
        withActiveCell { view, _, adapter ->
            tapProse(view)
            val root = view.richTextView.editorEditText
            val before = adapter.documentJson()
            pressTab(root)
            assertSame(root, view.richTextView.activeTextInput)
            assertEquals(before, adapter.documentJson())
        }

    @Test
    fun `cell input connection updates only the intended cell and emits a wrapper update`() =
        withActiveCell { view, input, adapter ->
            shadowOf(Looper.getMainLooper()).idle()
            val updates = mutableListOf<Map<String, Any>>()
            view.onEditorUpdateForTesting = updates::add
            val beforeRevision = adapter.baseDocumentRevision
            input.setSelection(input.text.length)
            val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(connection.commitText("X", 1))
            shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(200))
            assertEquals(listOf("FirstX", "Second"), cellTexts(adapter))
            assertEquals("after", proseText(adapter))
            assertEquals(beforeRevision + 1uL, adapter.baseDocumentRevision)
            assertTrue(updates.any {
                it["editorId"] == adapter.editorId &&
                    it["documentRevision"] == adapter.baseDocumentRevision.toString()
            })
        }

    @Test
    fun `read only invalidates the active cell connection`() = withActiveCell { view, input, adapter ->
        val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
        val documentBefore = adapter.documentJson()
        view.setEditable(false)
        assertSame(view.richTextView.editorEditText, view.richTextView.activeTextInput)
        assertTrue(!connection.beginBatchEdit())
        connection.commitText("wrong", 1)
        assertEquals(documentBefore, adapter.documentJson())
    }

    @Test
    fun `external composition begun in a cell does not edit root prose`() = withActiveCell { view, _, adapter ->
        val before = cellTexts(adapter)
        val result = JSONObject(view.beginExternalTextComposition("cell-speech"))
        assertEquals("EXTERNAL_COMPOSITION_UNAVAILABLE",
            result.getJSONObject("error").getString("code"))
        assertEquals(before, cellTexts(adapter))
        assertFalse(view.richTextView.editorEditText.hasActiveExternalTextCompositionForEditor())
    }

    @Test
    fun `native command preflight is blocked while a cell is active`() = withActiveCell { view, _, _ ->
        val result = JSONObject(view.prepareForEditorCommandJSON())
        assertFalse(result.getBoolean("ready"))
    }

    @Test
    fun `input props reach the mounted cell`() = withActiveCell { view, input, _ ->
        view.setKeyboardType("email-address")
        view.setAutoCapitalize("none")
        view.setAutoCorrect(false)
        view.setAndroidInputOptionsJson("""{"privateImeOptions":"table-test"}""")
        val info = EditorInfo()
        requireNotNull(input.onCreateInputConnection(info))
        assertEquals(InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
            info.inputType and InputType.TYPE_MASK_VARIATION)
        assertEquals("table-test", info.privateImeOptions)
        assertEquals(view.richTextView.editorEditText.inputType, input.inputType)
    }

    @Test
    fun `input props changed in prose reach the same reusable cell`() = withActiveCell { view, input, adapter ->
        val before = cellTexts(adapter)
        tapProse(view)
        assertSame(view.richTextView.editorEditText, view.richTextView.activeTextInput)
        view.setKeyboardType("email-address")
        view.setAutoCapitalize("none")
        view.setAutoCorrect(false)
        view.setAndroidInputOptionsJson("""{"privateImeOptions":"dormant-cell"}""")
        tapCell(view, 0)
        assertSame(input, view.richTextView.activeTextInput)
        val info = EditorInfo()
        requireNotNull(input.onCreateInputConnection(info))
        assertEquals(InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
            info.inputType and InputType.TYPE_MASK_VARIATION)
        assertEquals("dormant-cell", info.privateImeOptions)
        assertEquals(view.richTextView.editorEditText.inputType, input.inputType)
        assertEquals(before, cellTexts(adapter))
    }

    @Test
    fun `read only cell invalidation preserves focus when root receives it`() = withActiveCell { view, input, _ ->
        assertTrue(input.hasFocus())
        val changes = mutableListOf<Map<String, Any>>()
        view.onFocusChangeForTesting = changes::add
        view.setEditable(false)
        shadowOf(Looper.getMainLooper()).idle()
        assertSame(view.richTextView.editorEditText, view.richTextView.activeTextInput)
        assertTrue(view.richTextView.activeTextInput.hasFocus())
        assertFalse(changes.any { it["isFocused"] == false })
    }

    @Test
    fun `cell invalidation without replacement focus emits blur`() = withActiveCell { view, input, _ ->
        assertTrue(input.hasFocus())
        view.richTextView.editorEditText.isFocusable = false
        val changes = mutableListOf<Map<String, Any>>()
        view.onFocusChangeForTesting = changes::add
        view.setEditable(false)
        shadowOf(Looper.getMainLooper()).idle()
        assertFalse(view.richTextView.activeTextInput.hasFocus())
        assertEquals(listOf(false), changes.map { it["isFocused"] })
        assertFalse(view.isOutsideTapBlurHandlerInstalledForTesting())
    }

    @Test
    fun `switching cells does not emit editor focus loss`() = withActiveCell { view, _, _ ->
        val changes = mutableListOf<Map<String, Any>>()
        view.onFocusChangeForTesting = changes::add
        tapCell(view, 1)
        shadowOf(Looper.getMainLooper()).idle()
        assertFalse(changes.any { it["isFocused"] == false })
        assertTrue(view.richTextView.activeTextInput.hasFocus())
    }

    @Test
    fun `authoritative cell rectangle preserves wrapper focus and update shape`() =
        withActiveCell { view, input, adapter ->
            shadowOf(Looper.getMainLooper()).idle()
            val focusChanges = mutableListOf<Map<String, Any>>()
            val updates = mutableListOf<Map<String, Any>>()
            view.onFocusChangeForTesting = focusChanges::add
            view.onEditorUpdateForTesting = updates::add
            val staleConnection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            val before = adapter.documentJson()
            val cells = adapter.cachedTableRecords.values.single().getJSONArray("cells")
            fun point(index: Int): JSONObject {
                val opening = cells.getJSONObject(index).getInt("sourcePos")
                return JSONObject().put("offset", requireNotNull(adapter.scalarPositionForDoc(opening + 2)))
                    .put("kind", "scalar")
            }
            val selection = JSONObject().put("type", "cell")
                .put("anchorCell", point(0)).put("headCell", point(1))
            val result = adapter.callWithEnvelope(JSONObject().put("selection", selection)) {
                UniffiEditorV2Backend.setSelection(adapter.editorId, it)
            }
            assertTrue(result is EditorV2CallResult.Ok)
            assertTrue(view.richTextView.editorEditText.applyUpdateJSON(
                requireNotNull(adapter.refreshFromRustState(null))))
            shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(200))

            assertSame(view.richTextView.editorEditText, view.richTextView.activeTextInput)
            assertTrue(view.richTextView.activeTextInput.hasFocus())
            assertFalse(focusChanges.any { it["isFocused"] == false })
            assertTrue(view.isEditorEffectivelyFocusedForNativeAction())
            assertEquals(1, updates.size)
            val event = updates.single()
            assertEquals(adapter.editorId, event["editorId"])
            assertEquals(adapter.baseDocumentRevision.toString(), event["documentRevision"])
            val published = JSONObject(event["updateJson"] as String).getJSONObject("selection")
            assertEquals("cell", published.getString("type"))
            assertEquals(cells.getJSONObject(0).getInt("sourcePos"), published.getInt("anchorCell"))
            assertEquals(cells.getJSONObject(1).getInt("sourcePos"), published.getInt("headCell"))
            staleConnection.commitText("stale", 1)
            assertEquals(before, adapter.documentJson())
        }

    @Test
    fun `cell selection emits document coordinates`() = withActiveCell { view, input, adapter ->
        val events = mutableListOf<Map<String, Any>>()
        view.onSelectionChangeForTesting = events::add
        input.setSelection(1)
        val scalar = JSONObject(requireNotNull(adapter.selectionJson())).getInt("anchorScalar")
        val expected = requireNotNull(adapter.resolveSelectionMapping(scalar, scalar))[0]
        assertEquals(expected, events.last()["anchor"])
        assertEquals(expected, events.last()["head"])
    }

    @Test
    fun `wrapper selection state refresh keeps mounted cell input current`() =
        withActiveCell { _, input, adapter ->
            val boundEpoch = input.tableCellPositionMap?.binding?.epoch
            val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            input.setSelection(1)
            assertNotEquals(boundEpoch, adapter.positionEpoch)
            assertTrue(
                "cell map=${input.tableCellPositionMap?.binding} adapter epoch=${adapter.positionEpoch}",
                input.isAuthorizedForTableCellInput()
            )
            assertTrue(connection.commitText("Q", 1))
            assertEquals(listOf("FQirst", "Second"), cellTexts(adapter))
        }

    @Test
    fun `wrapper selection rebind cannot revive old cell connection`() =
        withActiveCell { view, input, adapter ->
            val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            val before = adapter.documentJson()
            view.onSelectionChangeForTesting = { view.setEditorId(0L) }
            input.setSelection(1)
            shadowOf(Looper.getMainLooper()).idle()
            assertSame(view.richTextView.editorEditText, view.richTextView.activeTextInput)
            assertFalse(input.isAuthorizedForTableCellInput())
            assertFalse(connection.beginBatchEdit())
            connection.commitText("wrong", 1)
            assertEquals(before, adapter.documentJson())
        }

    @Test
    fun `outside tap checks editor focus while a cell owns it`() = withActiveCell { view, input, _ ->
        assertTrue(input.hasFocus())
        assertTrue(view.isEditorFocusedForOutsideTapDecision())
        val point = IntArray(2)
        input.getLocationOnScreen(point)
        val event = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN,
            point[0] + 2f, point[1] + 2f, 0)
        try {
            assertEquals(NativeEditorOutsideTapDecision.PRESERVE_FOCUS,
                view.prepareOutsideTapDecisionForWindowEvent(event))
        } finally {
            event.recycle()
        }
    }

    @Test
    fun `native action scope changes when the reusable input moves to another cell`() =
        withActiveCell { view, input, _ ->
            input.setSelection(0)
            val action = NativeEditorExpoView.PendingNativeAction.ToolbarItemPress(
                NativeToolbarItem(type = ToolbarItemKind.ACTION, key = "custom", label = "Custom")
            )
            val first = view.currentNativeActionScope(action)
            tapCell(view, 1)
            assertSame(input, view.richTextView.activeTextInput)
            input.setSelection(0)
            assertFalse(view.isPendingNativeActionScopeCurrent(action, first))
        }

    private companion object {
        const val KEYBOARD_HEIGHT_PX = 600
        const val CELL_SELECTION_EXIT_RESTART = "restartInput:source=cellSelectionExit"
        const val ACTION_KEY = "insertSnippet"
        const val ACTION_LABEL = "Snippet"
        const val ACTION_TOOLBAR_JSON =
            """[{"type":"action","key":"$ACTION_KEY","label":"$ACTION_LABEL","icon":{"type":"glyph","text":"S"}}]"""
    }
}
