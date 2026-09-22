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
