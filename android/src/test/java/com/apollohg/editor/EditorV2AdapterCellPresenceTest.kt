package com.apollohg.editor

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

internal class RecordingAwarenessBackend : EditorV2Backend by UniffiEditorV2Backend {
    val selections = mutableListOf<JSONObject>()

    override fun collaborationSetAwarenessSelection(
        editorId: String,
        selectionJson: String
    ): EditorV2CallResult<String> {
        selections.add(JSONObject(selectionJson))
        return EditorV2CallResult.Ok("""{"outboundChanged":false}""")
    }
}

internal fun JSONObject.isCellPresence(anchor: Int, head: Int): Boolean =
    optString("type") == "cell" && optInt("anchorCell", -1) == anchor &&
        optInt("headCell", -1) == head && length() == 3

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class EditorV2AdapterCellPresenceTest {
    private val config = """{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"""
    private val document = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"""

    private fun EditorV2Adapter.selectCellsInEngine(anchor: Int, head: Int) {
        fun point(opening: Int) = JSONObject().put("kind", "document").put("offset", opening)
        val selection = JSONObject().put("type", "cell")
            .put("anchorCell", point(anchor)).put("headCell", point(head))
        val result = callWithEnvelope(JSONObject().put("selection", selection)) {
            UniffiEditorV2Backend.setSelection(editorId, it)
        }
        assertTrue("engine refused the cell selection: $result", result is EditorV2CallResult.Ok)
    }

    @Test
    fun adoptedCellSelectionsPublishCellPresenceUntilTextReplacesIt() {
        val backend = RecordingAwarenessBackend()
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(backend, JSONObject(created.value).getString("editorId"), true)
        )
        try {
            assertNotNull(adapter.setContentJson(document))
            val cells = adapter.cachedTableRecords.values.single().getJSONArray("cells")
            val first = cells.getJSONObject(0).getInt("sourcePos")
            val second = cells.getJSONObject(1).getInt("sourcePos")
            adapter.selectCellsInEngine(first, second)
            val render = UniffiEditorV2Backend.renderUpdate(adapter.editorId, null, null)
                as EditorV2CallResult.Ok
            backend.selections.clear()

            assertNotNull(adapter.adoptExternalRender(render.value))
            assertNotNull(adapter.adoptExternalRender(render.value))

            assertEquals("one publication per change: ${backend.selections}", 1, backend.selections.size)
            assertTrue("${backend.selections}", backend.selections.single().isCellPresence(first, second))

            assertNotNull(adapter.syncSelection(1, 1))

            assertEquals("text", backend.selections.last().optString("type"))
            assertNull(adapter.publishedCollaborationCells)
        } finally {
            adapter.destroy()
        }
    }
}
