package com.apollohg.editor

import com.apollohg.editor.tables.applyLocalSelection
import com.apollohg.editor.tables.required
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class ParagraphAlignmentContractTest : EditorV2AdapterTestFixture() {
    @Test
    fun `native render snapshots validate inherited explicit mixed and invalid alignment`() {
        val adapter = makeAdapter()
        assertNotNull(adapter.setContentHtml("<p>content</p>"))
        val original = requireNotNull(adapter.cachedAtomicRenderJson)
        for (alignment in listOf("left", "center", "right", "mixed", null)) {
            val snapshot = JSONObject(original)
            snapshot.getJSONObject("activeState").put("textAlign", alignment ?: JSONObject.NULL)
            assertNotNull(
                "active alignment=$alignment",
                parseAtomicRenderSnapshot(snapshot.toString())
            )
        }
        for (invalid in listOf("justify", "LEFT", 1, true)) {
            val snapshot = JSONObject(original)
            snapshot.getJSONObject("activeState").put("textAlign", invalid)
            assertNull(
                "invalid active alignment=$invalid",
                parseAtomicRenderSnapshot(snapshot.toString())
            )
        }
        for (alignment in listOf("left", "center", "right", null)) {
            val element = JSONObject().put("type", "blockStart").put("nodeType", "paragraph")
                .put("depth", 0).put("textAlign", alignment ?: JSONObject.NULL)
            assertTrue("block alignment=$alignment", validSemanticRenderElements(listOf(element)))
            element.put("textAlign", "justify")
            org.junit.Assert.assertFalse(validSemanticRenderElements(listOf(element)))
        }
    }

    @Test
    fun `alignment commands preserve selection and reject invalid values before engine dispatch`() {
        val adapter = makeAdapter()
        adapter.setContentHtml("<p>content</p>")
        val session = sessionOf(adapter)
        session.commands.clear()
        assertNotNull(adapter.setTextAlign("right", 1, 3))
        assertEquals("setTextAlign", session.commands.last().getString("type"))
        assertEquals("right", session.commands.last().getString("align"))
        assertEquals(
            1,
            requireNotNull(adapter.currentSelectionState()).selection.getInt("anchorScalar")
        )
        assertEquals(
            3,
            requireNotNull(adapter.currentSelectionState()).selection.getInt("headScalar")
        )
        assertNotNull(adapter.unsetTextAlign(1, 3))
        assertEquals("unsetTextAlign", session.commands.last().getString("type"))
        val count = session.commands.size
        assertNull(adapter.setTextAlign("justify", 0, 0))
        assertEquals(count, session.commands.size)
    }

    @Test
    fun `real engine alignment is admitted rendered and reset through native adapter`() {
        val created = UniffiEditorV2Backend.create(localEmptyConfig, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                UniffiEditorV2Backend,
                JSONObject(created.value).getString("editorId"),
                roomBound = false
            )
        )
        try {
            assertNotNull(adapter.setContentHtml("<p>content</p>"))
            val aligned = JSONObject(requireNotNull(adapter.setTextAlign("right", 1, 1)))
            assertEquals("right", aligned.getJSONObject("activeState").getString("textAlign"))
            val rendered = aligned.getJSONArray("renderBlocks").getJSONArray(0)
            assertEquals("right", rendered.getJSONObject(0).getString("textAlign"))
            val reset = JSONObject(requireNotNull(adapter.unsetTextAlign(1, 1)))
            assertTrue(reset.getJSONObject("activeState").isNull("textAlign"))
            val resetElements = reset.getJSONArray("renderBlocks").getJSONArray(0)
            assertTrue(resetElements.getJSONObject(0).isNull("textAlign"))
        } finally {
            adapter.destroy()
        }
    }

    @Test
    fun `native alignment command preserves selected table rectangle instead of scalar caret`() {
        val config = JSONObject(com.apollohg.editor.tables.PlainTableFixture.CONFIG)
        config.getJSONObject("schema").getJSONArray("nodes").getJSONObject(1)
            .put(
                "attrs",
                JSONObject().put("textAlign", JSONObject().put("default", JSONObject.NULL))
            )
        val created = UniffiEditorV2Backend.create(config.toString(), null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                UniffiEditorV2Backend,
                JSONObject(created.value).getString("editorId"),
                roomBound = false
            )
        )
        try {
            assertNotNull(
                adapter.setContentJson(com.apollohg.editor.tables.PlainTableFixture.document(2, 2))
            )
            val tableId = adapter.tableIndex.tableKeys.single()
            val anchor = requireNotNull(adapter.tableIndex.docStart(tableId, 0)).toInt()
            val head = requireNotNull(adapter.tableIndex.docStart(tableId, 3)).toInt()
            fun point(offset: Int) = JSONObject().put("kind", "document").put("offset", offset)
            assertTrue(
                adapter.applyLocalSelection(
                    JSONObject().put("type", "cell")
                        .put("anchorCell", point(anchor)).put("headCell", point(head))
                ) is EditorV2CallResult.Ok
            )
            assertNotNull(adapter.refreshFromRustState(null))
            val aligned = JSONObject(requireNotNull(adapter.setTextAlign("right", 0, 0)))
            assertEquals(
                "cell",
                requireNotNull(adapter.cachedAtomicRenderSelection()).getString("type")
            )
            assertEquals("right", aligned.getJSONObject("activeState").getString("textAlign"))
            val reset = JSONObject(requireNotNull(adapter.unsetTextAlign(0, 0)))
            assertEquals(
                "cell",
                requireNotNull(adapter.cachedAtomicRenderSelection()).getString("type")
            )
            assertTrue(reset.getJSONObject("activeState").isNull("textAlign"))
        } finally {
            adapter.destroy()
        }
    }

    @Test
    fun `native alignment keeps select all instead of replacing it with scalar caret`() {
        val created = UniffiEditorV2Backend.create(localEmptyConfig, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                UniffiEditorV2Backend,
                JSONObject(created.value).getString("editorId"),
                roomBound = false
            )
        )
        try {
            assertNotNull(adapter.setContentHtml("<p>first</p><h2>second</h2>"))
            assertTrue(
                adapter.applyLocalSelection(
                    JSONObject().put("type", "all")
                ) is EditorV2CallResult.Ok
            )
            assertNotNull(adapter.refreshFromRustState(null))
            val aligned = JSONObject(requireNotNull(adapter.setTextAlign("center", 0, 0)))
            assertEquals(
                "all",
                requireNotNull(adapter.cachedAtomicRenderSelection()).getString("type")
            )
            assertEquals("center", aligned.getJSONObject("activeState").getString("textAlign"))
            val reset = JSONObject(requireNotNull(adapter.unsetTextAlign(0, 0)))
            assertEquals(
                "all",
                requireNotNull(adapter.cachedAtomicRenderSelection()).getString("type")
            )
            assertTrue(reset.getJSONObject("activeState").isNull("textAlign"))
        } finally {
            adapter.destroy()
        }
    }

    @Test
    fun `alignment preserves text block node selection`() {
        val created = UniffiEditorV2Backend.create(localEmptyConfig, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                UniffiEditorV2Backend,
                JSONObject(created.value).getString("editorId"),
                roomBound = false
            )
        )
        val token = EditorV2Registry.register(adapter)
        val input = EditorEditText(org.robolectric.RuntimeEnvironment.getApplication())
        input.editorId = token
        input.v2Driver = adapter
        try {
            assertNotNull(adapter.setContentHtml("<p>first</p><h2>second</h2>"))
            val position = JSONObject().put(
                "kind",
                "document"
            ).put("offset", 0).put("affinity", "before")
            adapter.applyLocalSelection(
                JSONObject().put("type", "node")
                    .put("at", position)
            ).required("select paragraph node")
            assertNotNull(adapter.refreshFromRustState(null))
            val aligned = JSONObject(requireNotNull(adapter.setTextAlign("right", 7, 7)))
            val selection = requireNotNull(adapter.cachedAtomicRenderSelection())
            assertEquals("node", selection.getString("type"))
            assertEquals(0, selection.getInt("pos"))
            assertEquals(
                "Node endpoint must exclude following separator and heading",
                5,
                selection.getInt("endScalar")
            )
            assertTrue(input.applyUpdateJSON(aligned.toString()))
            input.applySelectionFromJSON(selection, adapter.baseDocumentRevision.toString())
            assertEquals(
                "first",
                input.text.toString().substring(input.selectionStart, input.selectionEnd)
            )
            input.syncCurrentSelectionToRust()
            assertEquals(
                "node",
                requireNotNull(adapter.cachedAtomicRenderSelection()).getString("type")
            )
            assertEquals("right", aligned.getJSONObject("activeState").getString("textAlign"))
            val document = JSONObject(requireNotNull(adapter.documentJson()))
            val content = document.getJSONArray("content")
            assertEquals(
                "right",
                content.getJSONObject(0).getJSONObject("attrs").getString("textAlign")
            )
            assertTrue(
                content.getJSONObject(1).optJSONObject("attrs")?.isNull("textAlign") != false
            )
            val reset = JSONObject(requireNotNull(adapter.unsetTextAlign(7, 7)))
            assertEquals(
                "node",
                requireNotNull(adapter.cachedAtomicRenderSelection()).getString("type")
            )
            assertTrue(reset.getJSONObject("activeState").isNull("textAlign"))
        } finally {
            input.v2Driver = null
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `empty table paragraph node endpoints match native cell projection after alignment`() {
        val config = JSONObject(com.apollohg.editor.tables.PlainTableFixture.CONFIG)
        config.getJSONObject("schema").getJSONArray("nodes").getJSONObject(1)
            .put(
                "attrs",
                JSONObject().put("textAlign", JSONObject().put("default", JSONObject.NULL))
            )
        val created = UniffiEditorV2Backend.create(config.toString(), null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                UniffiEditorV2Backend,
                JSONObject(created.value).getString("editorId"),
                roomBound = false
            )
        )
        try {
            assertNotNull(
                adapter.setContentJson(
                    com.apollohg.editor.tables.PlainTableFixture.document(1, 1, "")
                )
            )
            val tableId = adapter.tableIndex.tableKeys.single()
            val paragraphOpening =
                requireNotNull(adapter.tableIndex.docStart(tableId, 0)).toInt() + 1
            adapter.applyLocalSelection(
                JSONObject().put("type", "node").put(
                    "at",
                    JSONObject().put("kind", "document").put("offset", paragraphOpening)
                )
            )
                .required("select empty table paragraph node")
            assertNotNull(adapter.refreshFromRustState(null))
            assertNotNull(adapter.setTextAlign("right", 0, 0))
            val selection = requireNotNull(adapter.cachedAtomicRenderSelection())
            val block = requireNotNull(
                adapter.tableIndex.record(tableId)
            ).cells.single().inputBlocks.single()
            val origin = requireNotNull(adapter.tableIndex.scalarStart(tableId, 0)).toInt()
            assertEquals("node", selection.getString("type"))
            assertEquals(origin + block.contentScalarStart.toInt(), selection.getInt("posScalar"))
            assertEquals(origin + block.scalarEnd.toInt(), selection.getInt("endScalar"))
        } finally {
            adapter.destroy()
        }
    }
}
