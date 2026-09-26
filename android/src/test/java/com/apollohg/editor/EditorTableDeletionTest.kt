package com.apollohg.editor

import android.view.View
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class EditorTableDeletionTest {
    private class RecordingBackend : EditorV2Backend by UniffiEditorV2Backend {
        val commands = mutableListOf<JSONObject>()

        override fun applyCommand(editorId: String, requestJson: String): EditorV2CallResult<String> {
            commands += JSONObject(requestJson).getJSONObject("command")
            return UniffiEditorV2Backend.applyCommand(editorId, requestJson)
        }
    }

    private class Fixture(val view: RichTextEditorView, val adapter: EditorV2Adapter, val backend: RecordingBackend) {
        fun document(): JSONObject = JSONObject(requireNotNull(adapter.documentJson()))

        fun blockSummary(): List<String> = document().getJSONArray("content").let { content ->
            (0 until content.length()).map { index ->
                val block = content.getJSONObject(index)
                val text = block.optJSONArray("content")?.optJSONObject(0)?.optString("text").orEmpty()
                "${block.getString("type")}:$text"
            }
        }

        fun records(): List<JSONObject> = adapter.cachedTableRecords.values.sortedBy { it.getInt("tablePos") }

        fun apply(update: String?) {
            assertNotNull("the adapter produced no update", update)
            assertTrue(view.editorEditText.applyUpdateJSON(requireNotNull(update)))
        }
    }

    private fun withEditor(document: String, block: (Fixture) -> Unit) {
        val created = UniffiEditorV2Backend.create(TABLE_CONFIG, null) as EditorV2CallResult.Ok
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
            assertFalse("fixture must start without history", requireNotNull(adapter.historyCanUndo()))
            block(Fixture(view, adapter, backend))
        } finally {
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `delete table removes an empty frame by its position in one undoable mutation`() =
        withEditor(EMPTY_FRAME_DOCUMENT) { fixture ->
            val record = fixture.records().single()
            assertEquals("the fixture must be an empty frame: $record", 0, record.getInt("rows"))
            assertEquals("an empty frame has no cell to anchor a delete", 0, record.getJSONArray("cells").length())
            val before = fixture.document().toString()
            val admission = fixture.adapter.tableMutationAdmission("t${record.getInt("tablePos")}")

            fixture.apply(fixture.adapter.deleteTable(admission))

            assertEquals(
                "the engine receives one explicitly targeted delete",
                listOf(JSONObject().put("type", DELETE_TABLE).put("tablePos", record.getInt("tablePos")).toString()),
                fixture.backend.commands.map(JSONObject::toString)
            )
            assertEquals(REMAINING_PROSE, fixture.blockSummary())
            assertTrue("the frame's record is gone", fixture.adapter.cachedTableRecords.isEmpty())
            assertEquals(true, fixture.adapter.historyCanUndo())
            fixture.apply(fixture.adapter.undo())
            assertEquals("one undo restores the frame", before, fixture.document().toString())
            assertEquals("the delete was one history entry", false, fixture.adapter.historyCanUndo())
        }

    @Test
    fun `delete table refuses stale owner stale revision and nested admissions`() =
        withEditor(NESTED_DOCUMENT) { fixture ->
            val before = fixture.document().toString()
            val (outer, nested) = fixture.records()
            assertEquals(false, outer.getBoolean("readOnlyDescendants"))
            assertEquals(true, nested.getBoolean("readOnlyDescendants"))
            val outerId = "t${outer.getInt("tablePos")}"

            assertNull("a nested table is read-only",
                fixture.adapter.deleteTable(fixture.adapter.tableMutationAdmission("t${nested.getInt("tablePos")}")))

            val stale = fixture.adapter.tableMutationAdmission(outerId)
            fixture.apply(fixture.adapter.setContentJson(NESTED_DOCUMENT))
            assertNull("a stale revision must not delete", fixture.adapter.deleteTable(stale))

            val owned = fixture.adapter.tableMutationAdmission(outerId)
            assertTrue(fixture.adapter.admitsTableMutation(owned))
            fixture.adapter.releaseNativeBindingOwner(requireNotNull(fixture.adapter.currentNativeOwnerToken))
            assertNull("a lost owner must not delete", fixture.adapter.deleteTable(owned))

            assertTrue("no refused admission reaches the engine: ${fixture.backend.commands}",
                fixture.backend.commands.isEmpty())
            assertEquals(before, fixture.document().toString())
        }

    private companion object {
        const val VIEW_WIDTH = 480
        const val VIEW_HEIGHT = 320
        const val DELETE_TABLE = "deleteTable"
        const val TABLE_CONFIG = """{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"""
        const val EMPTY_FRAME_DOCUMENT = """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table"},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"""
        const val NESTED_DOCUMENT = """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Alpha"}]}]},{"type":"table_cell","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Nested"}]}]}]}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"""
        val REMAINING_PROSE = listOf("paragraph:before", "paragraph:after")
    }
}
