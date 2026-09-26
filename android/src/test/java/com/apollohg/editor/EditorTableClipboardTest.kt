package com.apollohg.editor

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.view.KeyEvent
import android.view.View
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
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
            val selection = JSONObject().put("type", "cell")
                .put("anchorCell", point(anchor)).put("headCell", point(head))
            val admitted = adapter.callWithEnvelope(JSONObject().put("selection", selection)) {
                UniffiEditorV2Backend.setSelection(adapter.editorId, it)
            }
            assertTrue("engine rejected the cell selection: $admitted", admitted is EditorV2CallResult.Ok)
            assertTrue(root.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
            assertTrue("root did not adopt the cell selection", root.authoritativeCellSelectionActive)
            backend.mutations.clear()
            updates.clear()
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
        block: (Fixture) -> Unit
    ) {
        val created = UniffiEditorV2Backend.create(schemaConfig, null) as EditorV2CallResult.Ok
        val backend = RecordingBackend()
        val adapter = requireNotNull(
            EditorV2Adapter.attach(backend, JSONObject(created.value).getString("editorId"), false)
        )
        val token = EditorV2Registry.register(adapter)
        try {
            val view = RichTextEditorView(RuntimeEnvironment.getApplication())
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
            block(Fixture(view, adapter, backend, updates))
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
    fun `nested read only cells copy but refuse cut and paste without mutation`() =
        withTable(EditorTableSurfaceMountTest.nestedTableDocument) { fixture ->
            val nested = fixture.openings(NESTED_TABLE)
            fixture.selectCells(nested[FIRST_CELL], nested[FIRST_CELL])
            val before = fixture.adapter.documentJson()

            assertTrue(fixture.root.onTextContextMenuItem(android.R.id.copy))
            assertEquals(NESTED_CELL_TEXT, requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString())
            assertTrue(fixture.root.onTextContextMenuItem(android.R.id.cut))
            clipboard().setPrimaryClip(ClipData.newPlainText(STALE_LABEL, PASTED_GRID_TSV))
            assertTrue(fixture.root.onTextContextMenuItem(android.R.id.paste))

            assertEquals(
                "both mutations must reach the planner and be refused there",
                listOf("$APPLY_COMMAND:$DELETE_BACKWARD_COMMAND", "$APPLY_COMMAND:$PASTE_COMMAND"),
                fixture.backend.mutations
            )
            assertEquals(before, fixture.adapter.documentJson())
            assertFalse(requireNotNull(fixture.adapter.historyCanUndo()))
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

    private companion object {
        const val APPLY_COMMAND = "applyCommand"
        const val APPLY_INPUT = "applyInput"
        const val SET_SELECTION = "setSelection"
        const val PASTE_COMMAND = "paste"
        const val DELETE_BACKWARD_COMMAND = "deleteBackward"
        const val CELL_SELECTION = "cell"
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
