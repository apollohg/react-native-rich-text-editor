package com.apollohg.editor

import com.apollohg.editor.tables.PlainTableFixture
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class EditorV2AdapterSelectionStateTest {
    private class SelectionStateBackend : EditorV2Backend by UniffiEditorV2Backend {
        var renderedFrames = 0
        var selectionStates = 0
        var transform: ((JSONObject) -> Unit)? = null
        var lastOutcome: JSONObject? = null

        override fun renderNativeFrame(
            editorId: String,
            ownerId: String?,
            mirrorAnchor: Int?,
            mirrorHead: Int?
        ): EditorV2CallResult<uniffi.editor_core.FfiNativeRenderFrame> {
            renderedFrames++
            return UniffiEditorV2Backend.renderNativeFrame(
                editorId,
                ownerId,
                mirrorAnchor,
                mirrorHead
            )
        }

        override fun applyNativeIntent(
            editorId: String,
            requestJson: String
        ): EditorV2CallResult<String> =
            when (val result = UniffiEditorV2Backend.applyNativeIntent(editorId, requestJson)) {
                is EditorV2CallResult.Err -> result

                is EditorV2CallResult.Ok -> {
                    val outcome = JSONObject(result.value)
                    if (outcome.has(SELECTION_STATE_KEY)) selectionStates++
                    transform?.invoke(outcome)
                    lastOutcome = outcome
                    EditorV2CallResult.Ok(outcome.toString())
                }
            }
    }

    private class AdoptedState(adapter: EditorV2Adapter) {
        val viewUpdate = requireNotNull(adapter.cachedViewUpdateJson)
        val atomicRender = requireNotNull(adapter.cachedAtomicRenderJson)
        val stateRevision = adapter.stateRevision
        val scalarLength = adapter.cachedScalarLength
        val authoritativeSelection = adapter.cachedAuthoritativeScalarSelection?.toList()
        val syncedSelection = adapter.lastSyncedScalarSelection?.toList()
        val activeState = adapter.cachedActiveState.toString()
        val historyState = adapter.cachedHistoryState.toString()
        val atomicSelection = adapter.cachedAtomicRenderSelectionObject.toString()
        val atomicRevision = adapter.cachedAtomicRenderDocumentRevision
        val positionEpoch = adapter.positionEpoch
    }

    @Test
    fun `text caret selection state matches the full render path`() =
        withSelectionStateAdapter { adapter, backend ->
            assertFastPathMatchesFullPath(adapter, backend, PROSE_CARET)
        }

    @Test
    fun `caret inside a table cell matches the full render path`() =
        withSelectionStateAdapter { adapter, backend ->
            val key = adapter.tableIndex.tableKeys.single()
            val cellStart = requireNotNull(adapter.tableIndex.scalarStart(key, 0)).toInt()
            assertFastPathMatchesFullPath(adapter, backend, cellStart + CELL_CARET_OFFSET)
        }

    @Test
    fun `keystroke after a selection state resolves against the kept epoch`() =
        withSelectionStateAdapter { adapter, backend ->
            val epoch = adapter.positionEpoch
            assertNotNull(adapter.syncSelection(PROSE_CARET, PROSE_CARET))
            assertEquals("the engine must offer selection state", 1, backend.selectionStates)
            assertEquals(epoch, adapter.positionEpoch)

            assertNotNull(adapter.insertText(INSERTED_TEXT, PROSE_CARET))

            assertEquals(
                "the next keystroke must not fall back to a position refresh",
                false,
                requireNotNull(backend.lastOutcome).getBoolean("positionFallback")
            )
            assertTrue(requireNotNull(adapter.documentHtml()).contains("befoxre the table"))
        }

    @Test
    fun `missing selection state takes the full render path`() =
        assertFullPathFallback { it.remove(SELECTION_STATE_KEY) }

    @Test
    fun `unresolvable cell selection takes the full render path`() = assertFullPathFallback {
        it.getJSONObject(SELECTION_STATE_KEY).put(
            "selection",
            JSONObject().put("type", "cell").put("anchorCell", MISSING_CELL)
                .put("headCell", MISSING_CELL)
        )
    }

    @Test
    fun `stale document version takes the full render path`() = assertFullPathFallback {
        val state = it.getJSONObject(SELECTION_STATE_KEY)
        state.put(
            "documentVersion",
            (state.getString("documentVersion").toULong() + 1u).toString()
        )
    }

    @Test
    fun `unexpected selection state key takes the full render path`() = assertFullPathFallback {
        it.getJSONObject(SELECTION_STATE_KEY).put("renderBlocks", JSONArray())
    }

    @Test
    fun `missing selection state key takes the full render path`() =
        assertFullPathFallback { it.getJSONObject(SELECTION_STATE_KEY).remove("documentIsEmpty") }

    @Test
    fun `malformed history state takes the full render path`() = assertFullPathFallback {
        it.getJSONObject(SELECTION_STATE_KEY).put(
            "historyState",
            JSONObject().put("canUndo", "no").put("canRedo", false)
        )
    }

    private fun assertFastPathMatchesFullPath(
        adapter: EditorV2Adapter,
        backend: SelectionStateBackend,
        scalar: Int
    ) {
        val retainedBlocks = adapter.cachedSemanticBlockTableIds
        val tableIndex = adapter.tableIndex
        val presentation = adapter.cachedTablePresentation
        val fullAdoptions = adapter.fullFrameAdoptionCountForTesting
        val deltaAdoptions = adapter.deltaFrameAdoptionCountForTesting
        val framesBefore = backend.renderedFrames
        val epochBefore = adapter.positionEpoch

        assertNotNull(adapter.syncSelection(scalar, scalar))

        assertEquals("the engine must offer selection state", 1, backend.selectionStates)
        assertEquals(
            "selection state must not render a frame",
            framesBefore,
            backend.renderedFrames
        )
        assertSame(retainedBlocks, adapter.cachedSemanticBlockTableIds)
        assertSame(tableIndex, adapter.tableIndex)
        assertSame(presentation, adapter.cachedTablePresentation)
        assertEquals(fullAdoptions, adapter.fullFrameAdoptionCountForTesting)
        assertEquals(deltaAdoptions, adapter.deltaFrameAdoptionCountForTesting)
        val fast = AdoptedState(adapter)
        assertEquals(listOf(scalar, scalar), fast.authoritativeSelection)
        assertEquals("selection state keeps the pinned epoch", epochBefore, fast.positionEpoch)

        adapter.invalidateCachedAtomicState(null)
        assertNotNull(adapter.refreshInternal(null, stripViewSelection = false))
        assertEquals(framesBefore + 1, backend.renderedFrames)
        val full = AdoptedState(adapter)

        assertEquals(JSONObject(full.atomicRender).toString(), fast.atomicRender)
        assertEquals(JSONObject(full.viewUpdate).toString(), fast.viewUpdate)
        assertEquals(full.stateRevision, fast.stateRevision)
        assertEquals(full.scalarLength, fast.scalarLength)
        assertEquals(full.authoritativeSelection, fast.authoritativeSelection)
        assertEquals(full.syncedSelection, fast.syncedSelection)
        assertEquals(full.activeState, fast.activeState)
        assertEquals(full.historyState, fast.historyState)
        assertEquals(full.atomicSelection, fast.atomicSelection)
        assertEquals(full.atomicRevision, fast.atomicRevision)
    }

    private fun assertFullPathFallback(corrupt: (JSONObject) -> Unit) =
        withSelectionStateAdapter { adapter, backend ->
            backend.transform = corrupt
            val framesBefore = backend.renderedFrames

            assertNotNull(adapter.syncSelection(PROSE_CARET, PROSE_CARET))

            assertEquals("the engine must offer selection state", 1, backend.selectionStates)
            assertEquals(
                "a rejected selection state must render the full frame",
                framesBefore + 1,
                backend.renderedFrames
            )
            assertEquals(
                listOf(PROSE_CARET, PROSE_CARET),
                adapter.cachedAuthoritativeScalarSelection?.toList()
            )
            assertTrue(
                "the full path must adopt a valid atomic snapshot",
                parseAtomicRenderSnapshot(requireNotNull(adapter.cachedAtomicRenderJson)) != null
            )
        }

    private fun withSelectionStateAdapter(test: (EditorV2Adapter, SelectionStateBackend) -> Unit) {
        val backend = SelectionStateBackend()
        val created = backend.create(PlainTableFixture.CONFIG, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                backend,
                JSONObject(created.value).getString("editorId"),
                roomBound = false
            )
        )
        val errors = mutableListOf<EditorV2Error>()
        adapter.bindAutonomousErrorOwner(OWNER_TOKEN, errors::add) {}
        try {
            assertNotNull(adapter.setContentJson(documentAroundTable()))
            assertNotNull(adapter.refreshFromRustState(null))
            test(adapter, backend)
            assertTrue(errors.toString(), errors.isEmpty())
        } finally {
            adapter.destroy()
        }
    }

    private fun documentAroundTable(): String {
        val table = JSONObject(PlainTableFixture.document(TABLE_ROWS, TABLE_COLUMNS))
            .getJSONArray("content").getJSONObject(0)
        fun paragraph(text: String) = JSONObject().put("type", "paragraph").put(
            "content",
            JSONArray().put(JSONObject().put("type", "text").put("text", text))
        )
        return JSONObject().put("type", "doc").put(
            "content",
            JSONArray().put(paragraph(PROSE_TEXT)).put(table).put(paragraph(PROSE_TEXT))
        ).toString()
    }

    private companion object {
        const val SELECTION_STATE_KEY = "selectionState"
        const val OWNER_TOKEN = 1L
        const val TABLE_ROWS = 2
        const val TABLE_COLUMNS = 2
        const val PROSE_TEXT = "before the table"
        const val PROSE_CARET = 4
        const val CELL_CARET_OFFSET = 3
        const val MISSING_CELL = 4096
        const val INSERTED_TEXT = "x"
    }
}
