package com.apollohg.editor
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
internal class EditorV2AdapterRenderUpdatesTest : EditorV2AdapterTestFixture() {
    private class FrameBackend : EditorV2Backend by UniffiEditorV2Backend {
        val frames = mutableListOf<uniffi.editor_core.FfiNativeRenderFrame>()
        var transform: ((uniffi.editor_core.FfiNativeRenderFrame) -> uniffi.editor_core.FfiNativeRenderFrame)? = null

        override fun renderNativeFrame(editorId: String, ownerId: String?, mirrorAnchor: Int?, mirrorHead: Int?): EditorV2CallResult<uniffi.editor_core.FfiNativeRenderFrame> {
            return when (val result = UniffiEditorV2Backend.renderNativeFrame(editorId, ownerId, mirrorAnchor, mirrorHead)) {
                is EditorV2CallResult.Err -> result
                is EditorV2CallResult.Ok -> {
                    frames.add(result.value)
                    EditorV2CallResult.Ok(transform?.invoke(result.value) ?: result.value)
                }
            }
        }
    }

    private fun withFrameAdapter(test: (EditorV2Adapter, FrameBackend, MutableList<EditorV2Error>) -> Unit) {
        val backend = FrameBackend()
        val created = backend.create(com.apollohg.editor.tables.PlainTableFixture.CONFIG, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(backend, JSONObject(created.value).getString("editorId"), roomBound = false))
        val errors = mutableListOf<EditorV2Error>()
        adapter.bindAutonomousErrorOwner(1L, errors::add) {}
        try {
            assertNotNull(adapter.setContentJson(com.apollohg.editor.tables.PlainTableFixture.document(2, 2)))
            backend.frames.clear()
            test(adapter, backend, errors)
        } finally {
            adapter.destroy()
        }
    }

    @Test
    fun `fresh binding expands retained root blocks without consuming another frame`() = withFrameAdapter { adapter, backend, errors ->
        val key = adapter.tableIndex.tableKeys.single()
        assertNotNull(adapter.insertText("X", requireNotNull(adapter.tableIndex.scalarStart(key, 0)).toInt()))
        adapter.releaseNativeBindingOwner(1L)
        backend.frames.clear()
        val token = EditorV2Registry.register(adapter)
        val input = EditorEditText(RuntimeEnvironment.getApplication())
        try {
            input.bindEditor(token, null)
            assertNotNull(input.rootTablePositionMap)
            assertEquals(listOf(uniffi.editor_core.FfiTableFrameKind.DELTA), backend.frames.map { it.tables.kind })
            assertTrue(errors.toString(), errors.isEmpty())
        } finally {
            input.unbindEditor()
            EditorV2Registry.remove(adapter.editorId)
        }
    }

    @Test
    fun `multi paragraph cell and nested table coordinates match the engine`() {
        val config = JSONObject(rootTableConfig)
        config.getJSONObject("schema").getJSONArray("nodes").put(JSONObject()
            .put("name", "mention").put("content", "").put("group", "inline").put("role", "inline")
            .put("isVoid", true).put("attrs", JSONObject().put("label", JSONObject().put("default", "Ada"))))
        val created = UniffiEditorV2Backend.create(config.toString(), null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(UniffiEditorV2Backend,
            JSONObject(created.value).getString("editorId"), false))
        try {
            val source = JSONObject(com.apollohg.editor.tables.PlainTableFixture.document(4, 3))
            val rows = source.getJSONArray("content").getJSONObject(0).getJSONArray("content")
            fun paragraph(text: String) = JSONObject().put("type", "paragraph").put("content",
                org.json.JSONArray().put(JSONObject().put("type", "text").put("text", text)))
            val rich = org.json.JSONArray().put(paragraph("first😀"))
                .put(paragraph("second").apply { getJSONArray("content").put(JSONObject().put("type", "mention")) })
                .put(paragraph("third"))
            rows.getJSONObject(1).getJSONArray("content").getJSONObject(1).put("content", rich)
            val nested = JSONObject(com.apollohg.editor.tables.PlainTableFixture.document(2, 2))
                .getJSONArray("content").getJSONObject(0)
            rows.getJSONObject(2).getJSONArray("content").getJSONObject(2).put("content",
                org.json.JSONArray().put(paragraph("before")).put(nested).put(paragraph("after")))
            assertNotNull(adapter.setContentJson(source.toString()))
            assertEquals(2, adapter.tableIndex.tableKeys.size)
            assertFramePositionsMatchEngine(adapter)
        } finally {
            adapter.destroy()
        }
    }

    @Test
    fun `destroy releases retained frame state`() = withFrameAdapter { adapter, _, _ ->
        assertFalse(adapter.tableIndex.tableKeys.isEmpty())
        assertNull(adapter.destroy())
        assertTrue(adapter.tableIndex.tableKeys.isEmpty())
        assertNull(adapter.installedFrameRevision)
        assertNull(adapter.cachedTablePresentation)
        assertNull(adapter.cachedSemanticRenderBlocks)
    }

    @Test
    fun `stale owner seed is a hint miss and refreshes the new owner without an error`() = withFrameAdapter { adapter, backend, oldErrors ->
        val changed = adapter.callWithEnvelope(JSONObject().put("text", "X")) { backend.applyInput(adapter.editorId, it) }
        assertTrue(changed is EditorV2CallResult.Ok)
        val newErrors = mutableListOf<EditorV2Error>()
        adapter.bindAutonomousErrorOwner(2L, newErrors::add) {}
        assertTrue(oldErrors.toString(), oldErrors.isEmpty())
        assertTrue(newErrors.toString(), newErrors.isEmpty())
        assertNotNull(adapter.refreshFromRustState(null))
        assertEquals(listOf(uniffi.editor_core.FfiTableFrameKind.FULL), backend.frames.map { it.tables.kind })
        assertTrue(newErrors.toString(), newErrors.isEmpty())
    }

    @Test
    fun `matching external reset adopts its one fetched frame`() = withFrameAdapter { adapter, backend, errors ->
        val reset = JSONObject().put("history", "resetAndClear")
            .put("documentRevision", adapter.baseDocumentRevision.toString())
            .put("setJson", JSONObject(requireNotNull(adapter.documentJson())))
        assertNotNull(adapter.adoptExternalReset(requireNotNull(adapter.cachedAtomicRenderJson), reset.toString()))
        assertEquals(listOf(uniffi.editor_core.FfiTableFrameKind.DELTA), backend.frames.map { it.tables.kind })
        assertTrue(errors.toString(), errors.isEmpty())
    }

    @Test
    fun `native keystroke adopts one changed cell and caches a table free commit snapshot`() = withFrameAdapter { adapter, backend, errors ->
        val key = adapter.tableIndex.tableKeys.single()
        val before = requireNotNull(adapter.tableIndex.record(key))
        val start = requireNotNull(adapter.tableIndex.scalarStart(key, 0)).toInt()
        assertNotNull(adapter.insertText("X", start))
        assertEquals(listOf(uniffi.editor_core.FfiTableFrameKind.DELTA), backend.frames.map { it.tables.kind })
        assertEquals(setOf(0), adapter.cachedTablePresentation?.changes?.changedCells?.get(key))
        val after = requireNotNull(adapter.tableIndex.record(key))
        assertEquals(before.cells[0].docSize + 1u, after.cells[0].docSize)
        assertEquals(before.cells.drop(1), after.cells.drop(1))
        val snapshot = requireNotNull(adapter.atomicRenderJson(adapter.baseDocumentRevision.toString()))
        assertEquals(backend.frames.single().snapshotJson, snapshot)
        for (legacyKey in listOf("tableAttributes", "tableRecords", "tableInputMappings")) {
            assertFalse(legacyKey, JSONObject(snapshot).has(legacyKey))
        }
        assertTrue(errors.toString(), errors.isEmpty())
    }

    @Test
    fun `stale frame base recovers with exactly one full frame`() = withFrameAdapter { adapter, backend, errors ->
        backend.transform = { frame ->
            if (backend.frames.size == 1) frame.copy(tables = frame.tables.copy(baseDocumentRevision = "0")) else frame
        }
        assertNotNull(adapter.refreshFromRustState(null))
        assertEquals(listOf(uniffi.editor_core.FfiTableFrameKind.DELTA, uniffi.editor_core.FfiTableFrameKind.FULL), backend.frames.map { it.tables.kind })
        assertEquals(adapter.baseDocumentRevision, adapter.installedFrameRevision)
        assertEquals(1, adapter.tableIndex.tableKeys.size)
        assertTrue(errors.toString(), errors.isEmpty())
    }

    @Test
    fun `corrupt full snapshot keeps root and index with one error`() = withFrameAdapter { adapter, backend, errors ->
        adapter.releaseNativeBindingOwner(1L)
        val key = adapter.tableIndex.tableKeys.single()
        val before = adapter.tableIndex.record(key)
        val root = adapter.cachedViewUpdateJson
        val atomic = adapter.cachedAtomicRenderJson
        backend.transform = { it.copy(snapshotJson = "{") }
        assertNull(adapter.refreshFromRustState(null))
        assertEquals(listOf(uniffi.editor_core.FfiTableFrameKind.FULL), backend.frames.map { it.tables.kind })
        assertEquals(root, adapter.cachedViewUpdateJson)
        assertEquals(atomic, adapter.cachedAtomicRenderJson)
        assertEquals(before, adapter.tableIndex.record(key))
        assertEquals(listOf("native table frame violates the frozen shape"), errors.map { it.message })
    }

    @Test
    fun `failed table full frame adopts without cells`() = withFrameAdapter { adapter, backend, errors ->
        adapter.releaseNativeBindingOwner(1L)
        backend.transform = { frame -> frame.copy(tables = frame.tables.copy(tables = frame.tables.tables.map {
            it.copy(failure = uniffi.editor_core.TableRenderFailure.GRID_LIMIT, cells = emptyList(), sourceRows = emptyList())
        })) }
        assertNotNull(adapter.refreshFromRustState(null))
        val record = requireNotNull(adapter.tableIndex.record(adapter.tableIndex.tableKeys.single()))
        assertEquals(uniffi.editor_core.TableRenderFailure.GRID_LIMIT, record.failure)
        assertTrue(record.cells.isEmpty())
        assertTrue(errors.toString(), errors.isEmpty())
    }

    @Test
    fun `reclaimed owner and refused cell backspace use delta frames`() = withFrameAdapter { adapter, backend, errors ->
        adapter.releaseNativeBindingOwner(1L)
        adapter.bindAutonomousErrorOwner(2L, errors::add) {}
        assertNotNull(adapter.refreshFromRustState(null))
        val key = adapter.tableIndex.tableKeys.single()
        val start = requireNotNull(adapter.tableIndex.scalarStart(key, 0)).toInt()
        assertNotNull(adapter.syncSelection(start, start))
        val before = adapter.documentJson()
        adapter.deleteBackwardAtSelection(start, start)
        assertEquals(before, adapter.documentJson())
        assertTrue(backend.frames.isNotEmpty())
        assertTrue(backend.frames.map { it.tables.kind }.toString(), backend.frames.all { it.tables.kind == uniffi.editor_core.FfiTableFrameKind.DELTA })
        assertTrue(errors.toString(), errors.isEmpty())
    }

    @Test
    fun `javascript render notice never installs javascript table content`() = withFrameAdapter { adapter, backend, errors ->
        val key = adapter.tableIndex.tableKeys.single()
        val before = adapter.tableIndex.record(key)
        val notice = JSONObject().put("documentVersion", adapter.baseDocumentRevision.toString())
            .put("tableRecords", JSONObject().put("forged", false))
            .put("renderBlocks", org.json.JSONArray().put(org.json.JSONArray().put(JSONObject())))
        assertNotNull(adapter.adoptExternalRender(notice.toString()))
        assertEquals(before, adapter.tableIndex.record(key))
        assertEquals(setOf(key), adapter.tableIndex.tableKeys.toSet())
        assertEquals(1, backend.frames.size)
        assertTrue(errors.toString(), errors.isEmpty())
    }

    private fun makeRootTableAdapter(): EditorV2Adapter {
        val created = UniffiEditorV2Backend.create(rootTableConfig, null) as EditorV2CallResult.Ok
        return requireNotNull(EditorV2Adapter.attach(UniffiEditorV2Backend,
            JSONObject(created.value).getString("editorId"), roomBound = false))
    }

    private fun rootTableDocument(after: String = "z"): String {
        val document = JSONObject(com.apollohg.editor.tables.PlainTableFixture.document(1, 1, "base"))
        document.getJSONArray("content").put(JSONObject().put("type", "paragraph").put("content",
            org.json.JSONArray().put(JSONObject().put("type", "text").put("text", after))))
        return document.toString()
    }

    @Test
    fun `root table input rejects index loss at unchanged revision and epoch`() {
        val adapter = makeRootTableAdapter()
        val token = EditorV2Registry.register(adapter)
        val input = EditorEditText(RuntimeEnvironment.getApplication())
        try {
            input.editorId = token
            input.v2Driver = adapter
            val update = requireNotNull(adapter.setContentJson(rootTableDocument()))
            assertTrue(input.applyUpdateJSON(update))
            input.applySelectionFromJSON(JSONObject().put("type", "text").put("anchor", 0).put("head", 0)
                .put("anchorScalar", 5).put("headScalar", 5), adapter.baseDocumentRevision.toString())
            assertEquals(5, input.inputScalar(2))
            val connection = requireNotNull(input.onCreateInputConnection(android.view.inputmethod.EditorInfo()))
            val before = input.text.toString()
            val epoch = adapter.positionEpoch
            adapter.tableIndex = com.apollohg.editor.tables.EditorTableIndex()
            assertEquals(epoch, adapter.positionEpoch)
            assertFalse(input.applyUpdateJSON(requireNotNull(adapter.cachedViewUpdateJson)))
            assertEquals(before, input.text.toString())
            assertNull(input.inputScalar(2))
            val documentBefore = adapter.documentJson()
            connection.commitText("wrong", 1)
            assertEquals(before, input.text.toString())
            assertEquals(documentBefore, adapter.documentJson())
        } finally {
            input.v2Driver = null
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `root table input rejects ownerless same revision readmission`() {
        val adapter = makeRootTableAdapter()
        val token = EditorV2Registry.register(adapter)
        val input = EditorEditText(RuntimeEnvironment.getApplication())
        try {
            input.editorId = token
            input.v2Driver = adapter
            val update = requireNotNull(adapter.setContentJson(rootTableDocument()))
            assertTrue(input.applyUpdateJSON(update))
            input.applySelectionFromJSON(JSONObject().put("type", "text").put("anchor", 0).put("head", 0)
                .put("anchorScalar", 5).put("headScalar", 5), adapter.baseDocumentRevision.toString())
            assertEquals(5, input.inputScalar(2))
            val connection = requireNotNull(input.onCreateInputConnection(android.view.inputmethod.EditorInfo()))
            val before = input.text.toString()
            adapter.releaseNativeBindingOwner(input.nativeBindingToken)
            assertNull(adapter.nativeOwnerId)
            assertNotNull(adapter.refreshFromRustState(null))
            assertNotNull(adapter.tableMappingsForTesting)
            assertNull(adapter.positionEpoch)
            assertFalse(input.ownsNativeBinding(adapter))
            assertNull(input.inputScalar(2))
            val documentBefore = adapter.documentJson()
            connection.commitText("wrong", 1)
            assertEquals(before, input.text.toString())
            assertEquals(documentBefore, adapter.documentJson())
        } finally {
            input.v2Driver = null
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `root table composition restores transient text when binding authority is lost`() {
        val adapter = makeRootTableAdapter()
        val token = EditorV2Registry.register(adapter)
        val input = EditorEditText(RuntimeEnvironment.getApplication())
        try {
            input.editorId = token
            input.v2Driver = adapter
            val update = requireNotNull(adapter.setContentJson(rootTableDocument()))
            assertTrue(input.applyUpdateJSON(update))
            input.applySelectionFromJSON(JSONObject().put("type", "text").put("anchor", 0).put("head", 0)
                .put("anchorScalar", 5).put("headScalar", 5), adapter.baseDocumentRevision.toString())
            val authorized = input.text.toString()
            input.setSelection(2)
            val connection = requireNotNull(input.onCreateInputConnection(
                android.view.inputmethod.EditorInfo()))
            assertTrue(connection.setComposingText("x", 1))
            assertEquals("\u200B\nxz", input.text.toString())
            adapter.releaseNativeBindingOwner(input.nativeBindingToken)
            assertFalse(input.ownsNativeBinding(adapter))
            val documentBefore = adapter.documentJson()
            assertTrue(connection.finishComposingText())
            assertEquals(authorized, input.text.toString())
            assertEquals(documentBefore, adapter.documentJson())
        } finally {
            input.v2Driver = null
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `root table marker maps following prose and blocks table input`() {
        val adapter = makeRootTableAdapter()
        val token = EditorV2Registry.register(adapter)
        val input = EditorEditText(RuntimeEnvironment.getApplication())
        try {
            input.editorId = token
            input.v2Driver = adapter
            val update = requireNotNull(adapter.setContentJson(rootTableDocument("😀z")))
            val tableKey = adapter.tableIndex.tableKeys.single()
            val probe = RenderBridge.buildSpannableFromBlocks(
                JSONObject(update).getJSONArray("renderBlocks"),
                baseFontSize = 16f,
                textColor = android.graphics.Color.BLACK,
                rootTableIds = setOf(tableKey)
            )
            assertEquals("\u200B\n😀z", probe.toString())
            assertNotNull(RootTablePositionMap.fromRendered(probe, mapOf(tableKey to TableScalarExtent(0, 4)), 7))
            assertEquals(update, adapter.cachedViewUpdateJson)
            assertEquals(1uL, adapter.cachedAtomicRenderDocumentRevision)
            assertNotNull(adapter.tableMappingsForTesting)

            val applied = input.applyUpdateJSON(update)
            assertTrue(input.imeTraceSnapshotForTesting().joinToString("\n"), applied)
            assertEquals("\u200B\n😀z", input.text.toString())
            input.applySelectionFromJSON(JSONObject().put("type", "text").put("anchor", 0).put("head", 0)
                .put("anchorScalar", 5).put("headScalar", 5), adapter.baseDocumentRevision.toString())
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
            val documentBefore = adapter.documentJson()
            staleConnection.commitText("wrong", 1)
            assertEquals(beforeStaleInput, input.text.toString())
            assertEquals(documentBefore, adapter.documentJson())
            adapter.baseDocumentRevision = 2uL
            assertNull(input.inputScalar(3))
        } finally {
            input.v2Driver = null
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `root table rejects an unpaired update atomically`() {
        val adapter = makeRootTableAdapter()
        val token = EditorV2Registry.register(adapter)
        val input = EditorEditText(RuntimeEnvironment.getApplication())
        try {
            input.editorId = token
            input.v2Driver = adapter
            val update = requireNotNull(adapter.setContentJson(rootTableDocument()))
            assertTrue(input.applyUpdateJSON(update))
            input.applySelectionFromJSON(JSONObject().put("type", "text").put("anchor", 0).put("head", 0)
                .put("anchorScalar", 5).put("headScalar", 5), adapter.baseDocumentRevision.toString())
            val before = input.text.toString()
            val stale = JSONObject(update).put("documentVersion", "0")
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

    private val rootTableConfig = """{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"hardBreak","content":"","group":"inline","role":"hardBreak","isVoid":true},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"""

    @Test
    fun `unchanged root blocks do not re-render when only extents change`() {
        val created = UniffiEditorV2Backend.create(rootTableConfig, null) as EditorV2CallResult.Ok
        val id = JSONObject(created.value).getString("editorId")
        val adapter = requireNotNull(EditorV2Adapter.attach(UniffiEditorV2Backend, id, roomBound = false))
        val input = EditorEditText(RuntimeEnvironment.getApplication()).apply {
            editorId = id.toLong()
            v2Driver = adapter
            captureApplyUpdateTraceForTesting = true
        }
        try {
            val document = """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"😀"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"""
            assertTrue(input.applyUpdateJSON(requireNotNull(adapter.setContentJson(document))))
            val originalText = input.text
            val originalMap = requireNotNull(input.rootTablePositionMap)
            val originalBlocks = input.currentRenderBlocksJson.toString()
            val proseLocal = input.text.toString().indexOf("after")
            val beforeScalar = requireNotNull(input.inputPositionScalarAtLocalUtf16(proseLocal, input.text.toString()))
            var rendered = 0
            input.onBeforeRenderRefresh = { rendered++ }
            val cell = requireNotNull(adapter.tableMappingsForTesting).tables.values.single().cells.single()
            val inserted = "XYZ"
            val update = requireNotNull(adapter.insertText(inserted, cell.blocks.first().contentScalarStart))
            assertTrue("a map-only table update is still adopted", input.applyUpdateJSON(update))
            assertEquals(originalBlocks, input.currentRenderBlocksJson.toString())
            assertTrue("root Editable must survive the extent-only update", input.text === originalText)
            assertEquals("root rendering must not run", 0, rendered)
            assertTrue(requireNotNull(input.lastApplyUpdateTraceForTesting).skippedRender)
            assertTrue("the map must adopt the new table extent", input.rootTablePositionMap !== originalMap)
            assertEquals(beforeScalar + inserted.length,
                input.inputPositionScalarAtLocalUtf16(proseLocal, input.text.toString()))
            assertEquals(adapter.baseDocumentRevision.toString(), input.rootTableMapDocumentVersion)
            assertEquals(adapter.positionEpoch, input.rootTableMapPositionEpoch)
        } finally {
            input.v2Driver = null
            adapter.destroy()
        }
    }

    @Test
    fun `real engine root table keeps following prose at its engine scalar`() {
        val created = UniffiEditorV2Backend.create(rootTableConfig, null) as EditorV2CallResult.Ok
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
            val extent = requireNotNull(adapter.tableMappingsForTesting?.tables?.values?.single()?.extent)
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
                val freshCreated = UniffiEditorV2Backend.create(rootTableConfig, null) as EditorV2CallResult.Ok
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
                val adjacent = freshAdapter.tableMappingsForTesting!!.tables.values.mapNotNull { it.extent }
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
                assertNull(freshAdapter.tableMappingsForTesting!!.tables.values.single().extent)
                assertNull(fresh.inputScalar(7))
            }

            withDocument(paragraph(""), table(true), paragraph("")) { freshAdapter, fresh ->
                val tableEnd = freshAdapter.tableMappingsForTesting!!.tables.values.single().extent!!.scalarEnd
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
                val tableEnd = freshAdapter.tableMappingsForTesting!!.tables.values.single().extent!!.scalarEnd
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
    fun `invalid root patch and frame leave all installed state untouched`() = withFrameAdapter { adapter, backend, errors ->
        val originalIndex = adapter.tableIndex
        val originalRoot = adapter.cachedViewUpdateJson
        val originalRevision = adapter.installedFrameRevision
        backend.transform = { frame -> frame.copy(snapshotJson = JSONObject(frame.snapshotJson)
            .put("renderBlocks", JSONObject.NULL).put("renderPatch", JSONObject()
                .put("baseDocumentVersion", "0").put("startIndex", 0).put("deleteCount", 1)
                .put("renderBlocks", org.json.JSONArray())).toString()) }
        assertNull(adapter.refreshFromRustState(null))
        assertTrue(originalIndex === adapter.tableIndex)
        assertEquals(originalRoot, adapter.cachedViewUpdateJson)
        assertEquals(originalRevision, adapter.installedFrameRevision)
        assertEquals(2, backend.frames.size)
        assertEquals(1, errors.size)
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

        backend.nextRenderUpdateResult = EditorV2CallResult.Ok(snapshot)
        assertNotNull(adapter.refreshFromRustState(null))
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

    private companion object {
        const val TABLE_SNAPSHOT_REVISION = 1uL
    }
}
