package com.apollohg.editor

import android.view.inputmethod.EditorInfo
import com.apollohg.editor.tables.EditorTableInputCoordinator
import com.apollohg.editor.tables.TableCellPositionMap
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorTableInputRoutingTest {
    private fun withCellInput(
        block: (EditorEditText, EditorTableInputCoordinator, EditorV2Adapter, FakeEditorV2Backend) -> Unit
    ) {
        val backend = FakeEditorV2Backend()
        val created = backend.create("""{"initialization":{"type":"localEmpty"}}""", null)
            as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(
            backend, JSONObject(created.value).getString("editorId"), roomBound = false
        ))
        val token = EditorV2Registry.register(adapter)
        val input = EditorEditText(RuntimeEnvironment.getApplication())
        val coordinator = EditorTableInputCoordinator(input)
        try {
            input.setText("a😀bc")
            input.lastAuthorizedText = "a😀bc"
            input.editorId = token
            input.v2Driver = adapter
            input.setSelection(3)
            adapter.baseDocumentRevision = 4uL
            adapter.positionEpoch = "9"
            val binding = TableCellPositionMap.Binding(10, "4", "9")
            val map = TableCellPositionMap(binding, listOf(TableCellPositionMap.Segment(0, 5, 40)))
            assertTrue(coordinator.bind(
                EditorTableInputCoordinator.Target(binding), map, "4", "9",
                authority = { true }, updateConsumer = { _, _, _ -> true }
            ))
            block(input, coordinator, adapter, backend)
        } finally {
            input.v2Driver = null
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    private fun withGappedCellInput(block: (EditorEditText, FakeEditorV2Backend) -> Unit) =
        withCellInput { input, coordinator, _, backend ->
            assertTrue(coordinator.invalidateBinding())
            val binding = TableCellPositionMap.Binding(10, "4", "9")
            val map = TableCellPositionMap(binding, listOf(
                TableCellPositionMap.Segment(0, 2, 40),
                TableCellPositionMap.Segment(2, 5, 50)
            ))
            assertTrue(coordinator.bind(
                EditorTableInputCoordinator.Target(binding), map, "4", "9",
                authority = { true }, updateConsumer = { _, _, _ -> true }
            ))
            block(input, backend)
        }

    @Test
    fun `cell input converts UTF-16 emoji coordinates and preserves reversed selection`() = withCellInput { input, _, _, _ ->
        assertEquals(42, input.inputScalarAtLocalUtf16(3, "a😀bc"))
        assertEquals(41 to 42, input.inputScalarRangeAtLocalUtf16(1, 3, "a😀bc"))
        assertEquals(43 to 41, input.inputScalarSelection(3, 1))
        assertEquals(3 to 1, input.localScalarSelection(43, 41))
        assertNull(input.inputScalarRange(4, 5))
        assertNull(input.inputScalar(-1))
    }

    @Test
    fun `cell typing uses global scalar and stale revision blocks the driver`() = withCellInput { input, _, adapter, backend ->
        val inserts = mutableListOf<Int>()
        input.onInsertTextInRustForTesting = { _, scalar -> inserts += scalar }
        assertEquals(3, input.selectionStart)
        assertEquals("a😀bc", input.text.toString())
        assertTrue(input.isAuthorizedForTableCellInput())
        assertEquals(42, input.inputScalar(2))
        input.handleTextCommit("X")
        assertEquals(input.imeTraceSnapshotForTesting().joinToString("\n"), listOf(42), inserts)

        input.onInsertTextInRustForTesting = null
        adapter.baseDocumentRevision = 5uL
        backend.calls.clear()
        input.handleTextCommit("Y")
        assertEquals(listOf(42), inserts)
        assertFalse(backend.calls.contains("applyNativeIntent"))
    }

    @Test
    fun `cell deletion and correction submit the global emoji range`() = withCellInput { input, _, _, _ ->
        val deleted = mutableListOf<Pair<Int, Int>>()
        val replaced = mutableListOf<Triple<Int, Int, String>>()
        input.onDeleteRangeInRustForTesting = { from, to -> deleted += from to to }
        input.onReplaceTextInRustForTesting = { from, to, value ->
            replaced += Triple(from, to, value)
        }

        input.handleDelete(2, 0)
        assertEquals(listOf(41 to 42), deleted)
        assertTrue(input.handleCorrectionCommit(1, 3, "😀", "Q"))
        assertEquals(listOf(Triple(41, 42, "Q")), replaced)
    }

    @Test
    fun `explicit correction rejects a cell range crossing a map gap`() = withGappedCellInput { input, backend ->
        val replaced = mutableListOf<Triple<Int, Int, String>>()
        input.onReplaceTextInRustForTesting = { from, to, value ->
            replaced += Triple(from, to, value)
        }
        backend.calls.clear()

        assertFalse(input.handleCorrectionCommit(1, 3, "😀", "Q"))

        assertTrue(replaced.isEmpty())
        assertFalse(backend.calls.contains("applyNativeIntent"))
    }

    @Test
    fun `inferred correction rejects a cell range crossing a map gap`() = withGappedCellInput { input, backend ->
        val replaced = mutableListOf<Triple<Int, Int, String>>()
        input.onReplaceTextInRustForTesting = { from, to, value ->
            replaced += Triple(from, to, value)
        }
        backend.calls.clear()

        assertFalse(input.handleMissingOldTextCorrectionCommit(1, 3, "😀", "Q"))

        assertTrue(replaced.isEmpty())
        assertFalse(backend.calls.contains("applyNativeIntent"))
    }

    @Test
    fun `cell paste submits a global caret position`() = withCellInput { input, _, _, _ ->
        val inserts = mutableListOf<Int>()
        input.onInsertTextInRustForTesting = { _, scalar -> inserts += scalar }
        input.pastePlainText("Q")
        assertEquals(listOf(42), inserts)
    }

    @Test
    fun `invalidated cell stays blocked until deliberate root bind`() = withCellInput { input, coordinator, _, backend ->
        val inserts = mutableListOf<Int>()
        input.onInsertTextInRustForTesting = { _, scalar -> inserts += scalar }
        input.setSelection(1)
        assertTrue(coordinator.invalidateBinding())
        assertNull(input.inputScalarAtLocalUtf16(1, "a😀bc"))
        backend.calls.clear()
        input.handleTextCommit("X")
        assertTrue(inserts.isEmpty())
        assertFalse(backend.calls.contains("applyNativeIntent"))
        backend.calls.clear()
        assertFalse(input.handleCopy())
        assertTrue(backend.calls.isEmpty())
        assertFalse(coordinator.invalidateBinding())
        input.unbindEditor()
        val visible = input.text.toString()
        input.handleTextCommit("Z")
        assertEquals(visible, input.text.toString())
    }

    @Test
    fun `incoming global text selection maps to local UTF-16 without losing direction`() = withCellInput { input, _, _, _ ->
        input.applySelectionFromJSON(
            JSONObject().put("type", "text")
                .put("anchor", 44).put("head", 42)
                .put("anchorScalar", 43).put("headScalar", 41),
            "4"
        )
        assertEquals(4, input.selectionStart)
        assertEquals(1, input.selectionEnd)
        assertEquals(3 to 1, input.currentLogicalScalarSelection())
    }

    @Test
    fun `false authority and changed epoch reject coordinates`() = withCellInput { input, _, adapter, backend ->
        input.tableCellInputAuthority = { false }
        assertNull(input.inputScalar(1))
        backend.calls.clear()
        input.handleTextCommit("X")
        assertFalse(backend.calls.contains("applyNativeIntent"))

        input.tableCellInputAuthority = { true }
        adapter.positionEpoch = "10"
        assertNull(input.inputScalar(1))
        input.handleTextCommit("Y")
        assertFalse(backend.calls.contains("applyNativeIntent"))
    }

    @Test
    fun `stale cell connection cannot compose or receive a root render`() = withCellInput { input, _, adapter, backend ->
        val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
        val initialText = input.text.toString()
        val rootUpdate = requireNotNull(adapter.currentStateJson())
        input.tableCellUpdateConsumer = { _, _, _ -> false }
        assertFalse(input.applyUpdateJSON(rootUpdate))
        assertEquals(initialText, input.text.toString())
        input.applyRenderJSON("[]")
        assertEquals(initialText, input.text.toString())
        assertNull(input.standaloneRenderJSON)

        adapter.positionEpoch = "10"
        backend.calls.clear()
        assertTrue(connection.setComposingText("wrong", 1))
        assertTrue(connection.commitText("wrong", 1))
        assertEquals(initialText, input.text.toString())
        assertFalse(backend.calls.contains("applyNativeIntent"))
    }

    @Test
    fun `root bind of same editor retires the cell connection`() = withCellInput { input, _, _, _ ->
        val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
        val generation = input.inputConnectionGenerationForTesting()
        input.bindEditor(input.editorId)
        assertFalse(input.isTableCellInput)
        assertTrue(input.inputConnectionGenerationForTesting() > generation)
        assertNull(input.activeInputConnection)
        assertTrue(connection.commitText("stale", 1))
    }

    @Test
    fun `rebind clears the prior logical selection snapshot`() = withCellInput { input, coordinator, _, _ ->
        input.rememberLogicalSelection(100, 100, 3, 3, "4")
        assertEquals(100 to 100, input.currentScalarSelection())
        assertTrue(coordinator.invalidateBinding())
        val binding = TableCellPositionMap.Binding(10, "4", "9")
        val reboundMap = TableCellPositionMap(
            binding, listOf(TableCellPositionMap.Segment(0, 5, 80))
        )
        assertTrue(coordinator.bind(
            EditorTableInputCoordinator.Target(binding), reboundMap, "4", "9",
            authority = { true }, updateConsumer = { _, _, _ -> true }
        ))
        assertEquals(2 to 2, input.currentScalarSelection())
    }

    @Test
    fun `registered adapter inserts at the mapped global position and delegates its update`() {
        val backend = FakeEditorV2Backend()
        val created = backend.create("""{"initialization":{"type":"localEmpty"}}""", null)
            as EditorV2CallResult.Ok
        val adapter = requireNotNull(EditorV2Adapter.attach(
            backend, JSONObject(created.value).getString("editorId"), roomBound = false
        ))
        val prefix = "x".repeat(40)
        requireNotNull(adapter.setContentHtml("<p>${prefix}abcde</p>"))
        val token = EditorV2Registry.register(adapter)
        val input = EditorEditText(RuntimeEnvironment.getApplication())
        try {
            input.setText("abcde")
            input.lastAuthorizedText = "abcde"
            input.editorId = token
            input.v2Driver = adapter
            input.setSelection(2)
            requireNotNull(adapter.currentStateJson())
            val revision = adapter.baseDocumentRevision.toString()
            val epoch = requireNotNull(adapter.positionEpoch)
            val binding = TableCellPositionMap.Binding(10, revision, epoch)
            val map = TableCellPositionMap(binding, listOf(TableCellPositionMap.Segment(0, 6, 40)))
            val updates = mutableListOf<String>()
            val coordinator = EditorTableInputCoordinator(input)
            assertTrue(coordinator.bind(
                EditorTableInputCoordinator.Target(binding), map, revision, epoch,
                authority = { true },
                updateConsumer = { update, _, _ -> updates += update; true }
            ))

            input.handleTextCommit("Q")

            val document = backend.getDocumentJson(adapter.editorId) as EditorV2CallResult.Ok
            assertEquals("${prefix}abQcde", FakeEditorV2Backend.documentTextOf(JSONObject(document.value)))
            assertEquals(1, updates.size)
            assertFalse(input.text.toString().startsWith(prefix))
        } finally {
            input.v2Driver = null
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }
}
