package com.apollohg.editor

import android.app.Activity
import android.graphics.Rect
import android.os.Looper
import android.text.InputType
import android.view.MotionEvent
import android.view.View
import android.view.inputmethod.EditorInfo
import android.widget.FrameLayout
import com.apollohg.editor.viewer.PreparedProseDrawingView
import java.time.Duration
import org.json.JSONObject
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

    private fun tapCell(view: NativeEditorExpoView, index: Int) {
        val canvas = (0 until view.richTextView.editorContentFrame.childCount)
            .map { view.richTextView.editorContentFrame.getChildAt(it) }
            .filterIsInstance<PreparedProseDrawingView>().single()
        val root = view.richTextView.editorEditText
        canvas.measure(View.MeasureSpec.makeMeasureSpec(root.width, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(root.height, View.MeasureSpec.EXACTLY))
        canvas.layout(0, 0, canvas.measuredWidth, canvas.measuredHeight)
        val table = requireNotNull(canvas.preparedLayout?.blocks?.singleOrNull())
        val cell = requireNotNull(table.tableSurface?.cells?.get(index))
        val bounds = requireNotNull(table.tableBounds)
        val canvasOrigin = Rect(0, 0, 1, 1)
        view.richTextView.offsetDescendantRectToMyCoords(canvas, canvasOrigin)
        val x = canvasOrigin.left + bounds.left + cell.frame.left + cell.contentOrigin.first + 8f
        val y = canvasOrigin.top + bounds.top + cell.frame.top + cell.contentOrigin.second + 8f
        val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
        val up = MotionEvent.obtain(0, 10, MotionEvent.ACTION_UP, x, y, 0)
        try {
            assertTrue(view.richTextView.dispatchTouchEvent(down))
            val handled = view.richTextView.dispatchTouchEvent(up)
            val adapter = root.v2Driver as? EditorV2Adapter
            assertTrue("root=${root.width}x${root.height} canvas=${canvas.width}x${canvas.height}" +
                " origin=$canvasOrigin tap=$x,$y revision=${adapter?.baseDocumentRevision}" +
                " applied=${root.lastAppliedDocumentVersion} epoch=${adapter?.positionEpoch}" +
                " owns=${adapter?.let(root::ownsNativeBinding)} mappings=${adapter?.cachedTableInputMappings?.tables?.keys}" +
                " rootMap=${root.rootTablePositionMap != null} rootTrace=${root.imeTraceSnapshotForTesting()}",
                handled)
        } finally {
            down.recycle()
            up.recycle()
        }
    }

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

    private fun withActiveCell(block: (NativeEditorExpoView, EditorEditText, EditorV2Adapter) -> Unit) {
        val activity = Robolectric.buildActivity(Activity::class.java).setup().get()
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(
            UniffiEditorV2Backend, JSONObject(created.value).getString("editorId"), false))
        val update = requireNotNull(adapter.setContentJson(document)) {
            adapter.debugNotes.toString()
        }
        val token = EditorV2Registry.register(adapter)
        try {
            val expo = testExpoContext(activity)
            val view = NativeEditorExpoView(expo.context, expo.appContext)
            view.onFocusChangeForTesting = {}
            view.onAddonEventForTesting = {}
            view.onEditorReadyForTesting = {}
            view.onEditorUpdateForTesting = {}
            view.onSelectionChangeForTesting = {}
            view.onContentHeightChangeForTesting = {}
            view.onAtomLayoutForTesting = {}
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
    fun `public focus keeps the active cell as input target`() = withActiveCell { view, input, _ ->
        input.clearFocus()
        view.focus()
        assertSame(input, view.richTextView.activeTextInput)
        assertTrue(input.hasFocus())
        assertTrue(view.isEditorEffectivelyFocusedForNativeAction())
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
}
