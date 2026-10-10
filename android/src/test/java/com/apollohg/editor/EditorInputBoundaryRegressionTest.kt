package com.apollohg.editor

import android.os.Looper
import android.os.SystemClock
import android.text.InputType
import android.text.TextUtils
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class EditorInputBoundaryRegressionTest : EditorInputConnectionTestFixture() {
    @Test
    fun `IME backspace through example content after card keeps atom deletable`() =
        assertExampleAtomTailDeletable(withPrecedingTable = false)

    @Test
    fun `IME backspace through example content with preceding table keeps atom deletable`() =
        assertExampleAtomTailDeletable(withPrecedingTable = true)

    @Test
    fun `settled IME backspace through example content with table keeps atom deletable`() =
        assertExampleAtomTailDeletable(withPrecedingTable = true, settleBetweenDeletes = true)

    @Test
    fun `IME backspace selects and deletes card after earlier table`() =
        assertExampleAtomTailDeletable(
            withPrecedingTable = true,
            tailDocument = EMPTY_ATOM_TAIL_DOCUMENT
        )

    @Test
    fun `code point backspace through example content with table keeps card deletable`() =
        assertExampleAtomTailDeletable(withPrecedingTable = true) {
            assertTrue(it.deleteSurroundingTextInCodePoints(1, 0))
        }

    @Test
    fun `hardware backspace through example content with table keeps card deletable`() {
        var eventTime = SystemClock.uptimeMillis()
        assertExampleAtomTailDeletable(withPrecedingTable = true) { input ->
            eventTime += HARDWARE_KEY_INTERVAL_MS
            assertTrue(
                input.sendKeyEvent(
                    KeyEvent(eventTime, eventTime, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DEL, 0)
                )
            )
            assertTrue(
                input.sendKeyEvent(
                    KeyEvent(eventTime, eventTime, KeyEvent.ACTION_UP, KeyEvent.KEYCODE_DEL, 0)
                )
            )
        }
    }

    private fun assertExampleAtomTailDeletable(
        withPrecedingTable: Boolean,
        settleBetweenDeletes: Boolean = false,
        tailDocument: String = EXAMPLE_ATOM_TAIL_DOCUMENT,
        backspace: (InputConnection) -> Unit = { assertTrue(it.deleteSurroundingText(1, 0)) }
    ) {
        val document = JSONObject(tailDocument)
        val blocks = document.getJSONArray("content")
        if (withPrecedingTable) {
            val withTable = org.json.JSONArray().put(
                blocks.getJSONObject(0)
            ).put(JSONObject(PRECEDING_TABLE))
            for (index in 1 until blocks.length()) withTable.put(blocks.getJSONObject(index))
            document.put("content", withTable)
        }
        withMountedEditorView(document.toString(), ATOM_CONFIG, windowed = true) {
                view,
                adapter,
                _
            ->
            val editor = view.editorEditText
            editor.applyAtomRenderConfiguration(COUNTER_CARD_CONFIGURATION)
            assertTrue(editor.requestFocus())
            val prefixBlocks = JSONObject(
                requireNotNull(adapter.documentJson())
            ).getJSONArray("content")
            val prefixLength = if (withPrecedingTable) 2 else 1
            val expectedPrefix = (0 until prefixLength).map {
                prefixBlocks.getJSONObject(it).toString()
            }
            editor.setSelection(editor.text!!.length)
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            val maximumBackspaces = editor.text!!.length + ATOM_BOUNDARY_BACKSPACE_ALLOWANCE
            var atomSelected = false
            for (step in 0 until maximumBackspaces) {
                val beforeDocument = adapter.documentJson()
                val beforeSelection = adapter.selectionJson()
                val beforeNativeSelection = editor.selectionStart to editor.selectionEnd
                backspace(input)
                if (settleBetweenDeletes) shadowOf(Looper.getMainLooper()).idle()
                val afterDocument = adapter.documentJson()
                val selection = JSONObject(requireNotNull(adapter.selectionJson()))
                val diagnostic = "step=$step document=$afterDocument selection=$selection " +
                    "native=${editor.text} " +
                    "caret=${editor.selectionStart}..${editor.selectionEnd}\n" +
                    editor.imeTraceSnapshotForTesting().joinToString("\n")
                if (selection.optString("type") == "node") {
                    val atom = editor.text!!.getSpans(
                        editor.selectionStart,
                        editor.selectionEnd,
                        AtomBlockSpan::class.java
                    ).firstOrNull()
                    if (atom?.nodeType == "counterCard") {
                        atomSelected = true
                        assertEquals(diagnostic, atom.docPos, selection.getInt("pos"))
                        assertFalse(diagnostic, editor.rootTableSelectionInputBlocked)
                        val blocks = JSONObject(
                            requireNotNull(afterDocument)
                        ).getJSONArray("content")
                        assertEquals(
                            diagnostic,
                            expectedPrefix.map { JSONObject(it).getString("type") } +
                                listOf("counterCard", "paragraph"),
                            (0 until blocks.length()).map {
                                blocks.getJSONObject(it).getString("type")
                            }
                        )
                        break
                    }
                }
                assertTrue(
                    "Backspace stuck after example CounterCard: $diagnostic",
                    afterDocument != beforeDocument || adapter.selectionJson() != beforeSelection ||
                        (editor.selectionStart to editor.selectionEnd) != beforeNativeSelection
                )
            }
            assertTrue("deleting the example's list and rule must reach CounterCard", atomSelected)
            backspace(input)
            assertFalse(requireNotNull(adapter.documentJson()).contains("counterCard"))
            assertTrue(input.commitText("again", 1))
            shadowOf(Looper.getMainLooper()).idle()
            assertTrue(editor.text.toString().endsWith("again"))
            assertEquals(editor.text!!.length, editor.selectionStart)
            assertEquals(editor.selectionStart, editor.selectionEnd)
            val blocks = JSONObject(requireNotNull(adapter.documentJson())).getJSONArray("content")
            assertEquals(
                expectedPrefix.map { JSONObject(it).getString("type") } + "paragraph",
                (0 until blocks.length()).map { blocks.getJSONObject(it).getString("type") }
            )
            assertEquals(
                expectedPrefix,
                (expectedPrefix.indices).map {
                    blocks.getJSONObject(it).toString()
                }
            )
            assertEquals(
                "again",
                blocks.getJSONObject(
                    blocks.length() - 1
                ).getJSONArray("content").getJSONObject(0).getString("text")
            )
        }
    }

    @Test
    fun `new unordered list item requests sentence capitalization`() {
        val harness = structuredDeleteHarness("<ul><li><p>First item</p></li></ul>")
        try {
            val editor = harness.editText
            editor.setAutoCapitalize("sentences")
            editor.setSelection(editor.text!!.length)
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            assertTrue(input.commitText("\n", 1))

            assertEquals(
                "text=${editor.text} selection=${editor.selectionStart}",
                InputType.TYPE_TEXT_FLAG_CAP_SENTENCES,
                input.getCursorCapsMode(InputType.TYPE_TEXT_FLAG_CAP_SENTENCES)
            )
            val context = requireNotNull(input.getTextBeforeCursor(100, 0))
            assertTrue(
                "IME context=$context",
                TextUtils.getCapsMode(
                    context,
                    context.length,
                    InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
                ) !=
                    0
            )
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `backspace selects then deletes atom after trailing empty paragraph`() {
        val harness = atomHarness()
        try {
            val editor = harness.editText
            editor.setSelection(editor.text!!.length)
            val scalar = requireNotNull(editor.currentScalarSelection())
            editor.applyUpdateJSON(
                requireNotNull(
                    harness.adapter.insertContentJsonAtSelection(
                        """{"type":"doc","content":[{"type":"counterCard"}]}""",
                        scalar.first,
                        scalar.second
                    )
                ),
                notifyListener = false
            )
            assertTrue(requireNotNull(harness.adapter.documentJson()).contains("counterCard"))
            val listener = RecordingEditorListener()
            val selectionStates = mutableListOf<String>()
            editor.editorListener = object : EditorEditText.EditorListener by listener {
                override fun onSelectionChanged(anchor: Int, head: Int) {
                    selectionStates += requireNotNull(harness.adapter.currentStateJson())
                }
            }
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            val beforeDeletion = harness.adapter.documentJson()

            assertTrue(input.deleteSurroundingText(1, 0))
            assertEquals(beforeDeletion, harness.adapter.documentJson())
            val atomOffset = editor.text.toString().indexOf('\uFFFC')
            assertEquals(atomOffset, editor.selectionStart)
            assertEquals(atomOffset + 1, editor.selectionEnd)
            val coreSelection = JSONObject(requireNotNull(harness.adapter.selectionJson()))
            assertEquals("node", coreSelection.getString("type"))
            assertTrue(listener.receivedUpdates.isEmpty())
            val emittedSelection = JSONObject(selectionStates.last()).getJSONObject("selection")
            assertEquals("node", emittedSelection.getString("type"))
            assertEquals(coreSelection.getInt("pos"), emittedSelection.getInt("pos"))
            assertTrue(input.deleteSurroundingText(1, 0))

            assertFalse(
                "selection=${editor.selectionStart}..${editor.selectionEnd} text=${editor.text}\n" +
                    editor.imeTraceSnapshotForTesting().joinToString("\n"),
                requireNotNull(harness.adapter.documentJson()).contains("counterCard")
            )
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `hardware backspace selects terminal atom in Rust before deleting it`() {
        val harness = atomHarness()
        try {
            val editor = harness.editText
            editor.applyUpdateJSON(
                requireNotNull(
                    harness.adapter.setContentJson(

                        """{"type":"doc","content":[{"type":"paragrap""" +
                            """h","content":[{"type":"text","text":"Before"}]},""" +
                            """{"type":"counterCard"},{"type":"paragraph"}]}"""
                    )
                ),
                notifyListener = false
            )
            editor.setSelection(editor.text!!.length)

            val beforeDeletion = harness.adapter.documentJson()
            editor.handleBackspace()
            assertEquals(beforeDeletion, harness.adapter.documentJson())

            val selection = JSONObject(requireNotNull(harness.adapter.selectionJson()))
            assertEquals("node", selection.getString("type"))
            val atomOffset = editor.text.toString().indexOf('\uFFFC')
            assertEquals(atomOffset, editor.selectionStart)
            assertEquals(atomOffset + 1, editor.selectionEnd)

            editor.handleBackspace()

            assertFalse(requireNotNull(harness.adapter.documentJson()).contains("counterCard"))
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `deleting inserted card preserves editable end after horizontal rule`() {
        val harness = atomHarness()
        try {
            val editor = harness.editText
            editor.applyUpdateJSON(
                requireNotNull(
                    harness.adapter.setContentJson(

                        """{"type":"doc","content":[{"type":"paragrap""" +
                            """h","content":[{"type":"text","text":"Before"}]},""" +
                            """{"type":"horizontalRule"},{"type":"paragraph"}]}"""
                    )
                ),
                notifyListener = false
            )
            editor.setSelection(editor.text!!.length)
            val scalar = requireNotNull(editor.currentScalarSelection())
            editor.applyUpdateJSON(
                requireNotNull(
                    harness.adapter.insertContentJsonAtSelection(
                        """{"type":"doc","content":[{"type":"counterCard"}]}""",
                        scalar.first,
                        scalar.second
                    )
                ),
                notifyListener = false
            )
            val deletionInput = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            assertTrue(deletionInput.deleteSurroundingText(1, 0))
            assertTrue(deletionInput.deleteSurroundingText(1, 0))
            editor.measure(
                View.MeasureSpec.makeMeasureSpec(320, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(400, View.MeasureSpec.EXACTLY)
            )
            editor.layout(0, 0, 320, 400)
            editor.setSelection(3)
            val y = (editor.totalPaddingTop + editor.layout.height + 30).toFloat()
            for ((action, time) in listOf(
                MotionEvent.ACTION_DOWN to 0L,
                MotionEvent.ACTION_UP to 16L
            )) {
                val event = MotionEvent.obtain(0, time, action, 30f, y, 0)
                editor.dispatchTouchEvent(event)
                event.recycle()
            }
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))

            assertTrue(input.commitText("Q", 1))

            val document = JSONObject(requireNotNull(harness.adapter.documentJson()))
            val blocks = document.getJSONArray("content")
            val lastBlock = blocks.getJSONObject(blocks.length() - 1)
            assertEquals(
                document.toString() + "\n" + editor.imeTraceSnapshotForTesting().joinToString("\n"),
                "paragraph",
                lastBlock.getString("type")
            )
            assertEquals("Q", lastBlock.getJSONArray("content").getJSONObject(0).getString("text"))
            assertEquals(
                "horizontalRule",
                blocks.getJSONObject(blocks.length() - 2).getString("type")
            )
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `selecting card publishes pending text deletion before selection event`() {
        val harness = atomHarness()
        try {
            val editor = harness.editText
            editor.applyUpdateJSON(
                requireNotNull(
                    harness.adapter.setContentJson(

                        """{"type":"doc","content":[{"type":"paragrap""" +
                            """h","content":[{"type":"text","text":"Before"}]},""" +
                            """{"type":"counterCard"},{"type":"paragrap""" +
                            """h","content":[{"type":"text","text":"x"}]}]}"""
                    )
                ),
                notifyListener = false
            )
            editor.setSelection(editor.text!!.length)
            val events = mutableListOf<String>()
            editor.editorListener = object : EditorEditText.EditorListener {
                override fun onSelectionChanged(anchor: Int, head: Int) {
                    events += "selection"
                }
                override fun onEditorUpdate(updateJSON: String) {
                    events += "commit"
                }
            }
            val input = requireNotNull(editor.onCreateInputConnection(EditorInfo()))
            assertTrue(input.deleteSurroundingText(1, 0))
            assertTrue(editor.hasDeferredRustUpdateApplicationForTesting())
            val afterTextDeletion = harness.adapter.documentJson()

            assertTrue(input.deleteSurroundingText(1, 0))

            assertEquals(afterTextDeletion, harness.adapter.documentJson())
            assertEquals(listOf("commit", "selection"), events)
            assertEquals(
                "node",
                JSONObject(requireNotNull(harness.adapter.selectionJson())).getString("type")
            )
        } finally {
            harness.adapter.destroy()
        }
    }

    @Test
    fun `IME backspace after horizontal rule keeps caret inside editable paragraph`() {
        assertBackspaceAfterHorizontalRule { editor ->
            assertTrue(
                requireNotNull(
                    editor.onCreateInputConnection(EditorInfo())
                ).deleteSurroundingText(1, 0)
            )
        }
    }

    @Test
    fun `code point backspace after horizontal rule keeps caret inside editable paragraph`() {
        assertBackspaceAfterHorizontalRule { editor ->
            assertTrue(
                requireNotNull(
                    editor.onCreateInputConnection(EditorInfo())
                ).deleteSurroundingTextInCodePoints(1, 0)
            )
        }
    }

    @Test
    fun `hardware backspace after horizontal rule keeps caret inside editable paragraph`() {
        assertBackspaceAfterHorizontalRule { it.handleBackspace() }
    }

    private fun assertBackspaceAfterHorizontalRule(backspace: (EditorEditText) -> Unit) {
        val harness = atomHarness()
        try {
            val editor = harness.editText
            editor.applyUpdateJSON(
                requireNotNull(
                    harness.adapter.setContentJson(

                        """{"type":"doc","content":[{"type":"paragrap""" +
                            """h","content":[{"type":"text","text":"Before"}]},""" +
                            """{"type":"horizontalRule"},{"type":"paragraph"}]}"""
                    )
                ),
                notifyListener = false
            )
            editor.setSelection(editor.text!!.length)
            backspace(editor)

            editor.measure(
                View.MeasureSpec.makeMeasureSpec(320, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(0, View.MeasureSpec.UNSPECIFIED)
            )
            editor.layout(0, 0, editor.measuredWidth, editor.measuredHeight)
            val caret = requireNotNull(editor.nativeCursorDrawRect())
            assertTrue(
                "caret=$caret layoutHeight=${editor.layout.height}",
                caret.bottom <= editor.layout.height
            )
            val document = requireNotNull(harness.adapter.documentJson())
            assertFalse(document, document.contains("horizontalRule"))
            assertEquals(
                "paragraph",
                JSONObject(document).getJSONArray("content").let {
                    it.getJSONObject(it.length() - 1).getString("type")
                }
            )
            assertTrue(
                requireNotNull(editor.onCreateInputConnection(EditorInfo())).commitText("Q", 1)
            )
            val blocks = JSONObject(
                requireNotNull(harness.adapter.documentJson())
            ).getJSONArray("content")
            assertEquals(
                "Q",
                blocks.getJSONObject(
                    blocks.length() - 1
                ).getJSONArray("content").getJSONObject(0).getString("text")
            )
        } finally {
            harness.adapter.destroy()
        }
    }

    private fun atomHarness(): RealExternalCompositionHarness = realExternalCompositionHarness(
        "Before",
        ATOM_CONFIG
    ).also {
        it.editText.applyUpdateJSON(
            requireNotNull(
                it.adapter.setContentJson(

                    """{"type":"doc","content":[{"type":"paragrap""" +
                        """h","content":[{"type":"text","text":"Before"}]}]}"""
                )
            ),
            notifyListener = false
        )
        it.editText.applyAtomRenderConfiguration(COUNTER_CARD_CONFIGURATION)
        it.adapter.claimNativeBindingIfUnowned(1L)
    }

    private companion object {
        val COUNTER_CARD_CONFIGURATION = AtomRenderConfiguration(
            setOf("counterCard"),
            mapOf("counterCard" to 72f),
            emptyMap()
        )
        val ATOM_CONFIG = """
            {
                "schema":{"nodes":[
                    {"name":"doc","content":"block+","role":"doc"},
                    {"name":"paragraph","content":"text*","group":"block","role":"textBlock"},
                    {"name":"text","content":"","role":"text"},
                    {"name":"counterCard","content":"","group":"block","role":"block","isVoid":true},
                    {"name":"horizontalRule","content":"","group":"block","role":"block","isVoid":true},
                    {"name":"horizontal_rule","content":"","group":"block","role":"block","isVoid":true,"htmlTag":"hr"},
                    {"name":"ordered_list","content":"list_item+","group":"block","role":"list","htmlTag":"ol"},
                    {"name":"list_item","content":"paragraph block*","role":"listItem"},
                    {"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table"},
                    {"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},
                    {"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},
                    {"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}
                ],"marks":[]},
                "initialization":{"type":"localEmpty"}
            }
        """.trimIndent()
        const val PRECEDING_TABLE = """
            {"type":"table","content":[{"type":"table_row","content":[
                {"type":"table_cell","content":[{"type":"paragraph","content":[
                    {"type":"text","text":"untouched table"}
                ]}]}
            ]}]}
        """

        const val ATOM_BOUNDARY_BACKSPACE_ALLOWANCE = 16
        const val HARDWARE_KEY_INTERVAL_MS = 10L
        const val EMPTY_ATOM_TAIL_DOCUMENT = """
            {"type":"doc","content":[
                {"type":"paragraph","content":[{"type":"text","text":"Before"}]},
                {"type":"counterCard"},{"type":"paragraph"}
            ]}
        """
        val EXAMPLE_ATOM_TAIL_DOCUMENT = """
            {"type":"doc","content":[
                {"type":"paragraph","content":[{"type":"text","text":"Before"}]},
                {"type":"counterCard"},
                {"type":"ordered_list","content":[
                    {"type":"list_item","content":[{"type":"paragraph","content":[{"type":"text","text":"Insert another with the + button"}]}]},
                    {"type":"list_item","content":[{"type":"paragraph","content":[{"type":"text","text":"Tap a counter to select it, then delete it like any block"}]}]}
                ]},
                {"type":"horizontal_rule"},
                {"type":"paragraph"}
            ]}
        """.trimIndent()
    }
}
