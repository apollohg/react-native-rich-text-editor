package com.apollohg.editor
import android.text.Spanned
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class EditorV2AdapterRenderUpdatesTest : EditorV2AdapterTestFixture() {
    @Test
    fun `table input mapping survives atomic and view snapshot adoption`() {
        val adapter = makeAdapter()
        val snapshot = tableInputMappingSnapshot()

        assertNotNull(adoptExternalRender(adapter, snapshot.toString()))
        assertEquals(1, adapter.cachedTableInputMappings?.tables?.get("t0")?.cells?.size)
        assertTrue(JSONObject(requireNotNull(adapter.cachedAtomicRenderJson)).has("tableInputMappings"))
        assertTrue(JSONObject(requireNotNull(adapter.cachedViewUpdateJson)).has("tableInputMappings"))
    }

    @Test
    fun `table input mapping rejects malformed associations atomically`() {
        val adapter = makeAdapter()
        val valid = tableInputMappingSnapshot()
        assertNotNull(adoptExternalRender(adapter, valid.toString()))
        val baseline = adapter.cachedAtomicRenderJson

        val orphaned = JSONObject(valid.toString())
        orphaned.getJSONObject("tableInputMappings").put("tables", JSONObject())
        assertNull(adoptExternalRender(adapter, orphaned.toString()))
        assertEquals(baseline, adapter.cachedAtomicRenderJson)

        val invalidCoordinates = JSONObject(valid.toString())
        val block = invalidCoordinates.getJSONObject("tableInputMappings").getJSONObject("tables")
            .getJSONObject("t0").getJSONArray("cells").getJSONObject(0).getJSONArray("blocks").getJSONObject(0)
        block.put("scalarEnd", 5)
        assertNull(adoptExternalRender(adapter, invalidCoordinates.toString()))
        assertEquals(baseline, adapter.cachedAtomicRenderJson)
    }

    @Test
    fun `legacy and release clear cached table input mapping`() {
        val adapter = makeAdapter()
        assertNotNull(adoptExternalRender(adapter, tableInputMappingSnapshot().toString()))
        assertNotNull(adapter.cachedTableInputMappings)

        val legacy = tableInputMappingSnapshot()
        legacy.remove("tableInputMappings")
        assertNotNull(adoptExternalRender(adapter, legacy.toString()))
        assertNull(adapter.cachedTableInputMappings)

        adapter.claimNativeBindingIfUnowned(1L)
        assertNull(adapter.cachedTableInputMappings)
        val native = tableInputMappingSnapshot().put("positionEpoch", "1")
        assertNotNull(adoptExternalRender(adapter, native.toString()))
        assertNotNull(adapter.cachedTableInputMappings)
        adapter.releaseNativeBindingOwner(1L)
        assertNull(adapter.cachedTableInputMappings)
    }

    private fun tableInputMappingSnapshot(): JSONObject {
        val attrsKey = "a".repeat(64)
        val table = JSONObject("""{
            "tablePos":0,"sourceEnd":12,"rows":1,"columns":1,"columnWidths":[null],
            "direction":null,"irregular":false,"readOnlyDescendants":false,"attrsKey":"$attrsKey",
            "sourceRows":[{"sourcePos":1,"sourceEnd":11,"attrsKey":"$attrsKey"}],
            "syntheticRegions":[],"failure":null,"compatibilityDiagnostic":null,
            "cells":[{"sourcePos":2,"sourceEnd":10,"row":0,"column":0,"rowspan":1,"colspan":1,
                "header":false,"attrsKey":"$attrsKey","contentKey":"cell","elements":[
                    {"type":"blockStart","nodeType":"paragraph","depth":0},
                    {"type":"textRun","text":"base","marks":[]},{"type":"blockEnd"}]}]
        }""")
        return JSONObject(atomicRenderSnapshot("base", "1"))
            .put("renderBlocks", org.json.JSONArray().put(org.json.JSONArray().put(JSONObject().put("type", "table").put("tableId", "t0"))))
            .put("tableAttributes", JSONObject().put(attrsKey, "{}"))
            .put("tableRecords", JSONObject().put("t0", table))
            .put("scalarLength", 4)
            .put("tableInputMappings", JSONObject().put("version", 1).put("tables", JSONObject().put("t0",
                JSONObject().put("extent", JSONObject().put("scalarStart", 0).put("scalarEnd", 4)).put("cells",
                    org.json.JSONArray().put(JSONObject().put("cellIndex", 0).put("sourcePos", 2).put("sourceEnd", 10)
                        .put("blocks", org.json.JSONArray().put(JSONObject().put("elementIndex", 0).put("docStart", 4).put("docEnd", 8)
                            .put("scalarStart", 0).put("contentScalarStart", 0).put("scalarEnd", 4).put("breakScalarEnd", 4).put("void", false)))
                        .put("excluded", org.json.JSONArray()))
                )
            )))
    }

    private fun tableInputMappingSnapshotWithFollowingProse(epoch: String): JSONObject =
        tableInputMappingSnapshot().apply {
            getJSONArray("renderBlocks").put(org.json.JSONArray()
                .put(JSONObject().put("type", "blockStart").put("nodeType", "paragraph").put("depth", 0))
                .put(JSONObject().put("type", "textRun").put("text", "z").put("marks", org.json.JSONArray()))
                .put(JSONObject().put("type", "blockEnd")))
            put("scalarLength", 6)
            getJSONObject("selection").put("anchorScalar", 5).put("headScalar", 5)
            put("positionEpoch", epoch)
        }

    @Test
    fun `root table input rejects sidecar loss at unchanged revision and epoch`() {
        val adapter = makeAdapter()
        val token = EditorV2Registry.register(adapter)
        val input = EditorEditText(RuntimeEnvironment.getApplication())
        try {
            input.editorId = token
            input.v2Driver = adapter
            val snapshot = tableInputMappingSnapshotWithFollowingProse(adapter.positionEpoch ?: "1")
            assertTrue(input.applyUpdateJSON(requireNotNull(adoptExternalRender(adapter, snapshot.toString()))))
            assertEquals(5, input.inputScalar(2))
            val connection = requireNotNull(input.onCreateInputConnection(android.view.inputmethod.EditorInfo()))
            val before = input.text.toString()
            val withoutMappings = JSONObject(snapshot.toString()).apply { remove("tableInputMappings") }
            assertNotNull(adoptExternalRender(adapter, withoutMappings.toString()))
            assertNull(adapter.cachedTableInputMappings)
            assertEquals(snapshot.getString("positionEpoch"), adapter.positionEpoch)
            assertFalse(input.applyUpdateJSON(requireNotNull(adapter.cachedViewUpdateJson)))
            assertEquals(before, input.text.toString())
            assertNull(input.inputScalar(2))
            backend.calls.clear()
            connection.commitText("wrong", 1)
            assertEquals(before, input.text.toString())
            assertFalse(backend.calls.contains("applyNativeIntent"))
        } finally {
            input.v2Driver = null
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `root table input rejects ownerless same revision readmission`() {
        val adapter = makeAdapter()
        val token = EditorV2Registry.register(adapter)
        val input = EditorEditText(RuntimeEnvironment.getApplication())
        try {
            input.editorId = token
            input.v2Driver = adapter
            val snapshot = tableInputMappingSnapshotWithFollowingProse(adapter.positionEpoch ?: "1")
            assertTrue(input.applyUpdateJSON(requireNotNull(adoptExternalRender(adapter, snapshot.toString()))))
            assertEquals(5, input.inputScalar(2))
            val connection = requireNotNull(input.onCreateInputConnection(android.view.inputmethod.EditorInfo()))
            val before = input.text.toString()
            adapter.releaseNativeBindingOwner(input.nativeBindingToken)
            assertNull(adapter.nativeOwnerId)
            assertNotNull(adoptExternalRender(adapter, snapshot.toString()))
            assertNotNull(adapter.cachedTableInputMappings)
            assertEquals(snapshot.getString("positionEpoch"), adapter.positionEpoch)
            assertFalse(input.ownsNativeBinding(adapter))
            assertNull(input.inputScalar(2))
            backend.calls.clear()
            connection.commitText("wrong", 1)
            assertEquals(before, input.text.toString())
            assertFalse(backend.calls.contains("applyNativeIntent"))
        } finally {
            input.v2Driver = null
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `root table composition restores transient text when binding authority is lost`() {
        val adapter = makeAdapter()
        val token = EditorV2Registry.register(adapter)
        val input = EditorEditText(RuntimeEnvironment.getApplication())
        try {
            input.editorId = token
            input.v2Driver = adapter
            val snapshot = tableInputMappingSnapshotWithFollowingProse(adapter.positionEpoch ?: "1")
            assertTrue(input.applyUpdateJSON(requireNotNull(adoptExternalRender(adapter, snapshot.toString()))))
            val authorized = input.text.toString()
            input.setSelection(2)
            val connection = requireNotNull(input.onCreateInputConnection(
                android.view.inputmethod.EditorInfo()))
            assertTrue(connection.setComposingText("x", 1))
            assertEquals("\u200B\nxz", input.text.toString())
            adapter.releaseNativeBindingOwner(input.nativeBindingToken)
            assertFalse(input.ownsNativeBinding(adapter))
            backend.calls.clear()
            assertTrue(connection.finishComposingText())
            assertEquals(authorized, input.text.toString())
            assertFalse(backend.calls.contains("applyNativeIntent"))
        } finally {
            input.v2Driver = null
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `root table marker maps following prose and blocks table input`() {
        val adapter = makeAdapter()
        val token = EditorV2Registry.register(adapter)
        val input = EditorEditText(RuntimeEnvironment.getApplication())
        try {
            input.editorId = token
            input.v2Driver = adapter
            val snapshot = tableInputMappingSnapshot()
            snapshot.getJSONArray("renderBlocks").put(org.json.JSONArray()
                .put(JSONObject().put("type", "blockStart").put("nodeType", "paragraph").put("depth", 0))
                .put(JSONObject().put("type", "textRun").put("text", "😀z").put("marks", org.json.JSONArray()))
                .put(JSONObject().put("type", "blockEnd")))
            snapshot.put("scalarLength", 7)
            snapshot.getJSONObject("selection").put("anchorScalar", 5).put("headScalar", 5)
            snapshot.put("positionEpoch", adapter.positionEpoch ?: "1")
            val update = requireNotNull(adoptExternalRender(adapter, snapshot.toString()))
            val probe = RenderBridge.buildSpannableFromBlocks(
                JSONObject(update).getJSONArray("renderBlocks"),
                baseFontSize = 16f,
                textColor = android.graphics.Color.BLACK,
                rootTableIds = setOf("t0")
            )
            assertEquals("\u200B\n😀z", probe.toString())
            assertNotNull(RootTablePositionMap.fromRendered(probe, mapOf("t0" to TableInputExtent(0, 4)), 7))
            assertEquals(update, adapter.cachedViewUpdateJson)
            assertEquals(1uL, adapter.cachedAtomicRenderDocumentRevision)
            assertNotNull(adapter.cachedTableInputMappings)

            val applied = input.applyUpdateJSON(update)
            assertTrue(input.imeTraceSnapshotForTesting().joinToString("\n"), applied)
            assertEquals("\u200B\n😀z", input.text.toString())
            assertEquals(5, input.inputScalarAtLocalUtf16(2, input.text.toString()))
            assertEquals(6, input.inputScalarAtLocalUtf16(4, input.text.toString()))
            assertEquals(5 to 6, input.inputScalarRangeAtLocalUtf16(2, 4, input.text.toString()))
            assertNull(input.inputScalar(0))
            assertNull(input.inputScalar(1))
            assertNull(input.inputScalarRange(0, 2))
            assertFalse(input.handleCorrectionCommit(0, 1, "\u200B", "Q"))
            assertFalse(input.handleMissingOldTextCorrectionCommit(0, 2, "\u200B\n", "Q"))
            input.applyRenderJSON("[]")
            assertEquals("\u200B\n😀z", input.text.toString())
            assertNull(input.localScalarSelection(0, 6))
            assertEquals(2 to 3, input.localScalarSelection(5, 6))
            input.applySelectionFromJSON(JSONObject().put("type", "text")
                .put("anchor", 0).put("head", 0)
                .put("anchorScalar", 2).put("headScalar", 2), "1")
            assertTrue(input.rootTableSelectionInputBlocked)
            assertNull(input.inputScalar(2))
            input.applySelectionFromJSON(JSONObject().put("type", "text")
                .put("anchor", 0).put("head", 0)
                .put("anchorScalar", 6).put("headScalar", 6), "1")
            assertFalse(input.rootTableSelectionInputBlocked)
            assertEquals(6, input.inputScalar(3))
            val staleConnection = requireNotNull(input.onCreateInputConnection(
                android.view.inputmethod.EditorInfo()))
            adapter.releaseNativeBindingOwner(input.nativeBindingToken)
            adapter.claimNativeBindingIfUnowned(input.nativeBindingToken + 1000)
            assertFalse(input.ownsNativeBinding(adapter))
            assertNull(input.inputScalar(3))
            val beforeStaleInput = input.text.toString()
            backend.calls.clear()
            staleConnection.commitText("wrong", 1)
            assertEquals(beforeStaleInput, input.text.toString())
            assertFalse(backend.calls.contains("applyNativeIntent"))
            adapter.baseDocumentRevision = 2uL
            assertNull(input.inputScalar(3))
        } finally {
            input.v2Driver = null
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `root table rejects an update without its mapping atomically`() {
        val adapter = makeAdapter()
        val token = EditorV2Registry.register(adapter)
        val input = EditorEditText(RuntimeEnvironment.getApplication())
        try {
            input.editorId = token
            input.v2Driver = adapter
            val update = requireNotNull(adoptExternalRender(adapter,
                tableInputMappingSnapshot().put("positionEpoch", adapter.positionEpoch ?: "1").toString()))
            assertTrue(input.applyUpdateJSON(update))
            val before = input.text.toString()
            val stale = JSONObject(update).apply { remove("tableInputMappings") }
            assertFalse(input.applyUpdateJSON(stale.toString()))
            assertEquals(before, input.text.toString())
        } finally {
            input.v2Driver = null
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `leading table composition rejection restores authorized render`() {
        val config = """{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"""
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val id = JSONObject(created.value).getString("editorId")
        val adapter = requireNotNull(EditorV2Adapter.attach(UniffiEditorV2Backend, id, roomBound = false))
        val input = EditorEditText(RuntimeEnvironment.getApplication()).apply {
            editorId = id.toLong()
            v2Driver = adapter
        }
        try {
            val document = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"z"}]}]}"""
            assertTrue(input.applyUpdateJSON(requireNotNull(adapter.setContentJson(document))))
            val authorized = input.text.toString()
            val documentBefore = adapter.documentJson()
            assertEquals("\u200B\nz", authorized)
            input.setSelection(2)
            assertEquals(5, input.inputScalarAtLocalUtf16(2, authorized))
            val connection = requireNotNull(input.onCreateInputConnection(
                android.view.inputmethod.EditorInfo()))
            assertTrue(connection.setComposingRegion(0, 0))
            assertTrue(connection.setComposingText("x", 1))
            assertTrue(connection.finishComposingText())
            assertEquals(documentBefore, adapter.documentJson())
            assertEquals(authorized, input.text.toString())
            assertEquals(5, input.inputScalarAtLocalUtf16(2, input.text.toString()))
        } finally {
            input.v2Driver = null
            adapter.destroy()
        }
    }

    @Test
    fun `real engine root table keeps following prose at its engine scalar`() {
        val config = """{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"hardBreak","content":"","group":"inline","role":"hardBreak","isVoid":true},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"""
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val id = JSONObject(created.value).getString("editorId")
        val adapter = requireNotNull(EditorV2Adapter.attach(UniffiEditorV2Backend, id, roomBound = false))
        val input = EditorEditText(RuntimeEnvironment.getApplication()).apply {
            editorId = id.toLong()
            v2Driver = adapter
        }
        try {
            val document = """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"😀"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"""
            val update = requireNotNull(adapter.setContentJson(document))
            assertTrue(input.applyUpdateJSON(update))
            val extent = requireNotNull(adapter.cachedTableInputMappings?.tables?.values?.single()?.extent)
            assertEquals(2, extent.scalarStart)
            assertEquals(6, extent.scalarEnd)
            assertEquals("😀\n\u200B\nafter", input.text.toString())
            assertEquals(7, input.inputScalarAtLocalUtf16(5, input.text.toString()))
            assertEquals(8, input.inputScalarAtLocalUtf16(6, input.text.toString()))
            assertNull(input.inputScalarRange(1, 5))
            input.setSelection(5)
            assertEquals("blocked=${input.rootTableSelectionInputBlocked} rev=${input.rootTableMapDocumentVersion}/${adapter.baseDocumentRevision} epoch=${input.rootTableMapPositionEpoch}/${adapter.positionEpoch}",
                7, input.inputScalar(4))
            val beforeBackspace = adapter.documentJson()
            val visibleBeforeBackspace = input.text.toString()
            val backspaceCalls = mutableListOf<Pair<Int, Int>>()
            input.onDeleteBackwardAtSelectionScalarInRustForTesting = { anchor, head ->
                backspaceCalls += anchor to head
            }
            val backspaceConnection = requireNotNull(input.onCreateInputConnection(
                android.view.inputmethod.EditorInfo()))
            assertTrue(backspaceConnection.deleteSurroundingText(1, 0))
            assertTrue(backspaceCalls.isEmpty())
            assertEquals(beforeBackspace, adapter.documentJson())
            assertEquals(visibleBeforeBackspace, input.text.toString())
            input.onDeleteBackwardAtSelectionScalarInRustForTesting = null
            val engineBackspaceConnection = requireNotNull(input.onCreateInputConnection(
                android.view.inputmethod.EditorInfo()))
            assertTrue(engineBackspaceConnection.deleteSurroundingText(1, 0))
            assertEquals(beforeBackspace, adapter.documentJson())
            assertEquals(visibleBeforeBackspace, input.text.toString())
            input.deleteBackwardAtSelectionScalarInRust(4, 4)
            assertEquals(beforeBackspace, adapter.documentJson())
            input.handleTextCommit("X")
            val changed = JSONObject(requireNotNull(adapter.documentJson())).getJSONArray("content")
            assertEquals(input.imeTraceSnapshotForTesting().joinToString("\n"), "Xafter", changed.getJSONObject(2).getJSONArray("content")
                .getJSONObject(0).getString("text"))
            assertEquals("cell", changed.getJSONObject(1).getJSONArray("content")
                .getJSONObject(0).getJSONArray("content").getJSONObject(0)
                .getJSONArray("content").getJSONObject(0)
                .getJSONArray("content").getJSONObject(0).getString("text"))

            fun paragraph(value: String) = JSONObject().put("type", "paragraph")
                .put("content", org.json.JSONArray().put(JSONObject().put("type", "text").put("text", value)))
            fun table(withCell: Boolean): JSONObject {
                val cells = org.json.JSONArray()
                if (withCell) cells.put(JSONObject().put("type", "table_cell")
                    .put("content", org.json.JSONArray().put(paragraph("cell"))))
                return JSONObject().put("type", "table").put("content", org.json.JSONArray()
                    .put(JSONObject().put("type", "table_row").put("content", cells)))
            }
            fun withDocument(vararg nodes: JSONObject, check: (EditorV2Adapter, EditorEditText) -> Unit) {
                val next = JSONObject().put("type", "doc").put("content", org.json.JSONArray().apply {
                    nodes.forEach(::put)
                })
                val freshCreated = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
                val freshId = JSONObject(freshCreated.value).getString("editorId")
                val freshAdapter = requireNotNull(EditorV2Adapter.attach(
                    UniffiEditorV2Backend, freshId, roomBound = false))
                val freshInput = EditorEditText(RuntimeEnvironment.getApplication()).apply {
                    editorId = freshId.toLong()
                    v2Driver = freshAdapter
                }
                try {
                    val freshUpdate = requireNotNull(freshAdapter.setContentJson(next.toString()))
                    val applied = freshInput.applyUpdateJSON(freshUpdate)
                    assertTrue(freshInput.imeTraceSnapshotForTesting().joinToString("\n"), applied)
                    check(freshAdapter, freshInput)
                } finally {
                    freshInput.v2Driver = null
                    freshAdapter.destroy()
                }
            }

            withDocument(paragraph("before"), table(true)) { _, fresh ->
                assertEquals("before\n\u200B", fresh.text.toString())
                assertEquals(6, fresh.inputScalar(6))
                assertNull(fresh.inputScalar(7))
            }

            withDocument(table(true), table(true), paragraph("end")) { freshAdapter, fresh ->
                assertEquals("\u200B\n\u200B\nend", fresh.text.toString())
                val adjacent = freshAdapter.cachedTableInputMappings!!.tables.values.mapNotNull { it.extent }
                    .sortedBy { it.scalarStart }
                assertEquals(2, adjacent.size)
                val after = adjacent[1].scalarEnd + 1
                fresh.applySelectionFromJSON(JSONObject().put("type", "text")
                    .put("anchor", 0).put("head", 0)
                    .put("anchorScalar", after).put("headScalar", after),
                    fresh.lastAppliedDocumentVersion)
                assertEquals(after,
                    fresh.inputScalarAtLocalUtf16(4, fresh.text.toString()))
                assertNull(fresh.inputScalarRange(0, 4))
            }

            withDocument(paragraph("before"), table(false), paragraph("after")) { freshAdapter, fresh ->
                assertEquals("before\nafter", fresh.text.toString())
                assertNull(freshAdapter.cachedTableInputMappings!!.tables.values.single().extent)
                assertNull(fresh.inputScalar(7))
            }

            withDocument(paragraph(""), table(true), paragraph("")) { freshAdapter, fresh ->
                val tableEnd = freshAdapter.cachedTableInputMappings!!.tables.values.single().extent!!.scalarEnd
                val marker = fresh.text.toString().indexOf('\u200B', 1)
                assertTrue(marker >= 0)
                assertEquals(tableEnd + 1, fresh.inputScalarAtLocalUtf16(marker + 2,
                    fresh.text.toString()))
            }

            val hardBreakParagraph = JSONObject().put("type", "paragraph")
                .put("content", org.json.JSONArray()
                    .put(JSONObject().put("type", "text").put("text", "tail"))
                    .put(JSONObject().put("type", "hardBreak")))
            withDocument(paragraph("before"), table(true), hardBreakParagraph) { freshAdapter, fresh ->
                val tableEnd = freshAdapter.cachedTableInputMappings!!.tables.values.single().extent!!.scalarEnd
                val after = fresh.text.toString().indexOf("tail")
                assertTrue(after >= 0)
                assertEquals(tableEnd + 1, fresh.inputScalarAtLocalUtf16(after, fresh.text.toString()))
            }
        } finally {
            input.v2Driver = null
            adapter.destroy()
        }
    }

    @Test
    fun `semantic table admission rejects malformed patches atomically`() {
        val adapter = makeAdapter()
        val attrsKey = "a".repeat(64)
        val snapshot = JSONObject(atomicRenderSnapshot("base", "1"))
        val table = JSONObject("""{
            "tablePos":0,"sourceEnd":10,"rows":1,"columns":1,"columnWidths":[null],
            "direction":null,"irregular":false,"readOnlyDescendants":false,"attrsKey":"$attrsKey",
            "sourceRows":[{"sourcePos":1,"sourceEnd":9,"attrsKey":"$attrsKey"}],
            "syntheticRegions":[],"failure":null,"compatibilityDiagnostic":null,
            "cells":[{"sourcePos":2,"sourceEnd":8,"row":0,"column":0,"rowspan":1,"colspan":1,
                "header":false,"attrsKey":"$attrsKey","contentKey":"same",
                "elements":[{"type":"textRun","text":"base","marks":[]}]}]
        }""")
        fun blocks(value: JSONObject) = org.json.JSONArray().put(org.json.JSONArray().put(JSONObject().put("type", "table").put("tableId", "t0")))
        snapshot.put("renderBlocks", blocks(table))
        snapshot.put("tableAttributes", JSONObject().put(attrsKey, "{}"))
        snapshot.put("tableRecords", JSONObject().put("t0", table))
        assertNotNull(adoptExternalRender(adapter, snapshot.toString()))
        val retainedNoop = JSONObject(snapshot.toString()).put("renderBlocks", JSONObject.NULL).put("renderPatch", JSONObject().put("baseDocumentVersion", "1")
            .put("startIndex", 0).put("deleteCount", 0).put("renderBlocks", org.json.JSONArray()))
        assertNotNull(adoptExternalRender(adapter, retainedNoop.toString()))
        val baseline = adapter.baseDocumentRevision
        val baselineJson = adapter.cachedAtomicRenderJson
        val missingRetainedReference = JSONObject(snapshot.toString()).put("renderBlocks", JSONObject.NULL)
            .put("tableAttributes", JSONObject()).put("renderPatch", JSONObject().put("baseDocumentVersion", "1")
                .put("startIndex", 0).put("deleteCount", 0).put("renderBlocks", org.json.JSONArray()))
        assertNull(adoptExternalRender(adapter, missingRetainedReference.toString()))
        assertEquals(baselineJson, adapter.cachedAtomicRenderJson)
        for (field in listOf("failure", "compatibilityDiagnostic", "columns", "attrsJson")) {
            val changed = JSONObject(table.toString()).put(field, when (field) {
                "columns" -> 0
                "attrsJson" -> "{\"width\":1e309}"
                else -> "unknown"
            })
            val patch = JSONObject(snapshot.toString()).put("renderBlocks", JSONObject.NULL).put("tableRecords", JSONObject().put("t0", changed)).put("renderPatch",
                JSONObject().put("baseDocumentVersion", "1").put("startIndex", 0).put("deleteCount", 1).put("renderBlocks", blocks(changed)))
            assertNull(adoptExternalRender(adapter, patch.toString()))
            assertEquals(baseline, adapter.baseDocumentRevision)
            assertEquals(baselineJson, adapter.cachedAtomicRenderJson)
        }
    }

    @Test
    fun `atomic render validation accepts an exclusive render patch`() {
        val adapter = makeAdapter()
        val snapshot = JSONObject(atomicRenderSnapshot("base", "1"))
        val blocks = snapshot.getJSONArray("renderBlocks")
        snapshot.put("renderBlocks", JSONObject.NULL)
        snapshot.put(
            "renderPatch",
            JSONObject()
                .put("baseDocumentVersion", "1")
                .put("startIndex", 0)
                .put("deleteCount", 0)
                .put("renderBlocks", blocks)
        )

        assertNotNull(adoptExternalRender(adapter, snapshot.toString()))
    }

    @Test
    fun `setContentHtml uses local API with reset history and renders derived blocks`() {
        val adapter = makeAdapter()
        val update = adapter.setContentHtml("<p>Hello</p>")
        assertEquals("Hello", renderedText(update))
        assertEquals(1uL, adapter.baseDocumentRevision)
        val parsed = JSONObject(requireNotNull(update))
        assertEquals(1, parsed.getInt("documentVersion"))
        assertFalse(parsed.getJSONObject("historyState").getBoolean("canUndo"))
        assertEquals("Hello", documentText(adapter))
    }

    @Test
    fun `move selection constructs a native structural command`() {
        val adapter = makeAdapter()
        adapter.setContentHtml("<p>abcd</p>")
        adapter.claimNativeBindingIfUnowned(1L)

        assertNotNull(adapter.moveSelection(0, 2, 4))

        val command = sessionOf(adapter).commands.last()
        assertEquals("moveSelection", command.getString("type"))
        assertEquals(0, command.getJSONObject("range").getJSONObject("from").getInt("offset"))
        assertEquals(2, command.getJSONObject("range").getJSONObject("to").getInt("offset"))
        assertEquals(4, command.getJSONObject("at").getInt("offset"))
    }

    @Test
    fun `refresh reports a failed engine render update`() {
        // A render update that fails used to return null in silence, so no
        // caller — the paired view or the stateless render probe — could name
        // the cause.
        val adapter = makeAdapter()
        adapter.setContentHtml("<p>base</p>")
        val errors = mutableListOf<EditorV2Error>()
        adapter.onAutonomousError = { errors.add(it) }

        // Destroy the session behind the adapter's back: the next render
        // update reaches a handle the backend no longer knows.
        backend.destroy(adapter.editorId)

        assertNull(adapter.refreshFromRustState(mirrorSelection = null))
        assertEquals(1, errors.size)
        assertEquals("lifecycle", errors.single().domain)
    }

    @Test
    fun `backward selection position mapping is exact`() {
        val adapter = makeAdapter()
        adapter.setContentHtml("<p>abcd</p>")
        val mapping = adapter.syncSelection(3, 1)
        assertNotNull(mapping)
        assertEquals(4, mapping!!.docAnchor)
        assertEquals(2, mapping.docHead)

        val update = adapter.replaceTextRange(1, 3, "X")
        assertEquals("aXd", renderedText(update))
        assertEquals("aXd", documentText(adapter))
    }

    @Test
    fun `range deletion render carries post-delete caret`() {
        val adapter = makeAdapter()
        adapter.setContentHtml("<p>abcd</p>")

        val deleted = adapter.deleteScalarRange(1, 3)

        assertEquals("ad", renderedText(deleted))
        val selection = JSONObject(requireNotNull(deleted)).getJSONObject("selection")
        assertEquals(1, selection.getInt("anchorScalar"))
        assertEquals(1, selection.getInt("headScalar"))
    }

    @Test
    fun `native deletion remains applied after post mutation render recovery`() {
        val adapter = makeAdapter()
        adapter.setContentHtml("<p>abcd</p>")
        adapter.claimNativeBindingIfUnowned(99L)
        assertNotNull(adapter.currentStateJson())
        backend.nextRenderUpdateResult = EditorV2CallResult.Err(
            EditorV2Error("render", "RENDER_FAILED", "transient")
        )

        val outcome = adapter.deleteScalarRangeNative(1, 2)

        assertTrue(outcome is EditorV2NativeIntentResult.Applied)
        val recovery = (outcome as EditorV2NativeIntentResult.Applied).render.updateJson
        assertTrue(outcome.render.documentChanged)
        assertEquals("acd", renderedText(recovery))
        val selection = JSONObject(recovery).getJSONObject("selection")
        assertEquals(1, selection.getInt("anchorScalar"))
        assertEquals(1, selection.getInt("headScalar"))
    }

    @Test
    fun `resize image retains the engine node selection in its update`() {
        val adapter = makeAdapter()
        adapter.setContentHtml("<p>HelloX</p>")
        backend.nextRenderUpdateResult = EditorV2CallResult.Ok(
            imageAtomicRenderSnapshot(revision = "2", width = 120)
        )

        val update = JSONObject(requireNotNull(adapter.resizeImageAtDocPos(7, 120, 80)))

        assertTrue("Resize update must retain the engine selection", update.has("selection"))
        val selection = update.optJSONObject("selection")
        assertNotNull(selection)
        selection ?: return
        assertEquals("node", selection.getString("type"))
        assertEquals(7, selection.getInt("pos"))
    }

    @Test
    fun `render update carries blocks active state and native input post caret`() {
        val adapter = makeAdapter()
        adapter.setContentHtml("<p>ab</p>")
        backend.calls.clear()
        val update = JSONObject(requireNotNull(adapter.insertText("c", 2)))
        assertTrue(update.has("renderBlocks"))
        assertTrue(update.has("activeState"))
        // The scalar extent rides the accessor payload but stays
        // adapter-internal (the view-facing update keeps the legacy shape).
        assertFalse(update.has("scalarLength"))
        // History/version are the v2 engine's facts, never the fake's
        // deliberately wrong sentinels.
        assertEquals(2, update.getInt("documentVersion"))
        val history = update.getJSONObject("historyState")
        assertTrue(history.getBoolean("canUndo"))
        assertFalse(history.getBoolean("canRedo"))
        // A full render replacement resets Android's selection. The native input update must
        // therefore carry the adapter's authoritative post-input scalar caret.
        val selection = update.getJSONObject("selection")
        assertEquals("text", selection.getString("type"))
        assertEquals(3, selection.getInt("anchorScalar"))
        assertEquals(3, selection.getInt("headScalar"))
        assertTrue(backend.calls.contains("renderUpdate"))
    }

    @Test
    fun `render update mirrors scalar selection to doc positions`() {
        val adapter = makeAdapter()
        val update = JSONObject(
            requireNotNull(adapter.setContentHtml("<p>ab</p><p>cd</p>"))
        )
        val selection = update.getJSONObject("selection")
        assertEquals("text", selection.getString("type"))
        assertEquals(0, selection.getInt("anchorScalar"))
        assertEquals(0, selection.getInt("headScalar"))
        assertEquals(1, selection.getInt("anchor"))
        assertEquals(1, selection.getInt("head"))
    }

    @Test
    fun `empty document refresh carries blocks and authoritative selection`() {
        val adapter = makeAdapter()
        val update = JSONObject(requireNotNull(adapter.currentStateJson()))
        assertTrue(update.has("renderBlocks"))
        assertTrue(update.has("activeState"))
        assertFalse(update.has("scalarLength"))
        val selection = update.getJSONObject("selection")
        assertEquals("text", selection.getString("type"))
        assertEquals(0, selection.getInt("anchorScalar"))
        assertEquals(0, selection.getInt("headScalar"))
        assertEquals(1, selection.getInt("anchor"))
        assertEquals(1, selection.getInt("head"))
        assertEquals(0, update.getInt("documentVersion"))
    }

    @Test
    fun `a second mismatch after recovery returns a refresh without another retry`() {
        val adapter = makeAdapter()
        adapter.setContentHtml("<p>base</p>")
        adapter.syncSelection(0, 0)
        val session = sessionOf(adapter)
        session.text.append("R")
        session.revision += 1u
        backend.advanceRevisionAfterNextRender = true
        backend.calls.clear()

        val update = adapter.insertText("X", 2)

        assertNotNull(update)
        assertEquals("baseR", documentText(adapter))
        assertEquals(0, backend.calls.count { it == "applyInput" })
        assertEquals(1, backend.calls.count { it == "renderUpdate" })
    }

    @Test
    fun `split renders refuse a stale split and mark not applicable uncommitted`() {
        val adapter = makeAdapter()
        adapter.setContentHtml("<p>base</p>")
        adapter.syncSelection(0, 0)
        val session = sessionOf(adapter)
        session.text.insert(0, "REMOTE ")
        session.revision += 1u

        backend.calls.clear()
        val stale = adapter.splitBlockAt(0)

        assertNotNull(stale)
        assertFalse(stale!!.committed)
        assertEquals("REMOTE base", renderedText(stale.updateJson))
        assertEquals(1, backend.calls.count { it == "applyCommand" })
        assertEquals(1, backend.calls.count { it == "renderUpdate" })

        adapter.setContentHtml("<p>next</p>")
        adapter.syncSelection(0, 1)
        backend.forceNextSplitCommandNotApplicableWithRemoteText("REMOTE")
        backend.calls.clear()
        val notApplicable = adapter.deleteAndSplit(0, 1)

        assertNotNull(notApplicable)
        assertFalse(notApplicable!!.committed)
        assertEquals("REMOTE", renderedText(notApplicable.updateJson))
        assertEquals(1, backend.calls.count { it == "applyCommand" })
        assertEquals(1, backend.calls.count { it == "renderUpdate" })
    }

    @Test
    fun `atomic adoption serves authoritative selection and history without split reads`() {
        val adapter = makeAdapter()
        adapter.setContentHtml("<p>ab</p>")
        val session = sessionOf(adapter)
        val snapshot = JSONObject(
            atomicRenderSnapshot("ab", session.revision.toString(), selectionScalar = 2)
        )
            .put("historyState", JSONObject().put("canUndo", false).put("canRedo", true))
            .toString()

        assertNotNull(adoptExternalRender(adapter, snapshot))
        backend.calls.clear()

        assertEquals(false, adapter.historyCanUndo())
        assertEquals(true, adapter.historyCanRedo())
        val state = JSONObject(requireNotNull(adapter.currentStateJson()))
        assertEquals(2, state.getJSONObject("selection").getInt("anchorScalar"))
        assertEquals(
            2,
            JSONObject(requireNotNull(adapter.selectionJson())).getInt("anchorScalar")
        )
        assertEquals(0, backend.calls.count { it == "getState" })
    }

    @Test
    fun `local selection and mutation replace adopted authoritative caches coherently`() {
        val adapter = makeAdapter()
        adapter.setContentHtml("<p>ab</p>")
        assertNotNull(adapter.syncSelection(1, 1))
        val session = sessionOf(adapter)
        session.anchor = 2
        session.head = 2
        session.revision += 1u
        val externalSnapshot =
            atomicRenderSnapshot("ab", session.revision.toString(), selectionScalar = 2)
        assertNotNull(adoptExternalRender(adapter, externalSnapshot))

        backend.calls.clear()
        assertNotNull(adapter.syncSelection(1, 1))
        assertTrue(backend.calls.contains("setSelection"))

        backend.calls.clear()
        val updated = adapter.insertText("x", 1)
        assertEquals("axb", renderedText(updated))
        assertEquals(true, adapter.historyCanUndo())
        assertEquals(false, adapter.historyCanRedo())
        assertEquals(0, backend.calls.count { it == "getState" })
        assertEquals(
            2,
            JSONObject(requireNotNull(adapter.selectionJson())).getInt("anchorScalar")
        )
    }

    @Test
    fun `request envelopes carry version request id and base revision`() {
        val adapter = makeAdapter()
        adapter.setContentHtml("<p>ab</p>")
        backend.calls.clear()
        adapter.insertText("X", 2)
        // The fake asserts the envelope on admission; this test pins the
        // exact revision arithmetic: base revision is the pre-commit one.
        val session = sessionOf(adapter)
        assertEquals(2uL, session.revision)
        assertEquals(2uL, adapter.baseDocumentRevision)
    }
}
