package com.apollohg.editor

import android.content.ClipboardManager
import android.content.Context
import android.graphics.Color
import android.view.MotionEvent
import android.view.inputmethod.EditorInfo
import com.apollohg.editor.viewer.PreparedProseDrawingView
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class RichTextEditorViewLazySurfaceTest {
    @Test
    fun `table free editing never creates the table surface or overlays`() = withMountedEditorView(
        PLAIN_DOCUMENT,
        MOUNTED_TABLE_EDITOR_CONFIG,
        windowed = true
    ) { view, _, _ ->
        val root = view.editorEditText
        root.requestFocus()
        root.setSelection(root.text.length)
        val input = requireNotNull(root.onCreateInputConnection(EditorInfo()))
        assertTrue(input.commitText(TYPED_TEXT, 1))
        root.setSelection(1)
        root.setSelection(0, SELECTED_LENGTH)
        tapAt(view, TAP_INSET, TAP_INSET)
        view.setRemoteSelections(emptyList())
        measureMountedEditor(view, EDITOR_WIDTH, EDITOR_HEIGHT)

        assertEquals(PLAIN_TEXT + TYPED_TEXT, root.text.toString())
        assertNull(
            "a table-free session must not build the table surface",
            view.existingTableSurface
        )
        assertFalse(view.hasRemoteSelectionOverlayForTesting)
        assertFalse(view.hasImageResizeOverlayForTesting)
        assertNull(tableDrawing(view))
        assertSame(root, view.activeTextInput)
    }

    @Test
    fun `table loaded through setContent brings up a working table surface`() =
        assertTableSurfaceComesUp { view, _ ->
            view.setContent(JSONObject(MOUNTED_TABLE_DOCUMENT))
        }

    @Test
    fun `inserted table brings up a working table surface`() =
        assertTableSurfaceComesUp { view, adapter ->
            val root = view.editorEditText
            val (anchor, head) = requireNotNull(root.currentScalarSelection())
            val update = adapter.insertContentJsonAtSelection(TABLE_FRAGMENT, anchor, head)
            assertTrue(root.applyUpdateJSON(requireNotNull(update)))
        }

    @Test
    fun `pasted table brings up a working table surface`() = assertTableSurfaceComesUp { view, _ ->
        withMountedEditorView(
            MOUNTED_TABLE_DOCUMENT,
            MOUNTED_TABLE_EDITOR_CONFIG
        ) { _, source, _ ->
            assertNotNull(
                source.syncSelection(0, requireNotNull(source.cachedScalarLength))
            )
            val payload = EditorClipboard.fromExportJson(requireNotNull(source.clipboardJson()))
            val clipboard = RuntimeEnvironment.getApplication()
                .getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            clipboard.setPrimaryClip(EditorClipboard.create(requireNotNull(payload)))
        }
        assertTrue(view.editorEditText.onTextContextMenuItem(android.R.id.paste))
    }

    @Test
    fun `externally synced table brings up a working table surface`() =
        assertTableSurfaceComesUp { view, adapter ->
            val update = replaceTableDocumentExternallyForTest(adapter, MOUNTED_TABLE_DOCUMENT)
            assertTrue(
                view.editorEditText.applyUpdateJSON(
                    update,
                    refreshInputConnectionForExternalUpdate = true
                )
            )
        }

    @Test
    fun `remote selection overlay is created by the first remote selection`() =
        withMountedEditorView(PLAIN_DOCUMENT, MOUNTED_TABLE_EDITOR_CONFIG) { view, _, _ ->
            view.setRemoteSelections(emptyList())
            view.refreshRemoteSelections()
            assertFalse(
                "an empty remote selection list needs no overlay",
                view.hasRemoteSelectionOverlayForTesting
            )

            view.setRemoteSelectionScalarResolverForTesting { _, docPos -> docPos }
            view.setRemoteSelections(
                listOf(
                    RemoteSelectionDecoration(
                        clientId = REMOTE_CLIENT_ID,
                        anchor = REMOTE_CARET,
                        head = REMOTE_CARET,
                        color = Color.BLUE,
                        name = REMOTE_NAME,
                        isFocused = true
                    )
                )
            )

            assertTrue(view.hasRemoteSelectionOverlayForTesting)
            assertEquals(
                REMOTE_CLIENT_ID,
                view.remoteSelectionDebugSnapshotsForTesting().single().clientId
            )
            assertNull(view.existingTableSurface)
        }

    private fun assertTableSurfaceComesUp(addTable: (RichTextEditorView, EditorV2Adapter) -> Unit) =
        withMountedEditorView(
            PLAIN_DOCUMENT,
            MOUNTED_TABLE_EDITOR_CONFIG,
            windowed = true
        ) { view, adapter, _ ->
            view.editorEditText.requestFocus()
            view.editorEditText.setSelection(view.editorEditText.text.length)
            assertNull(view.existingTableSurface)

            addTable(view, adapter)
            measureMountedEditor(view, EDITOR_WIDTH, EDITOR_HEIGHT)

            assertTrue(
                "the document must hold a table: ${adapter.documentJson()}",
                adapter.tableIndex.tableKeys.isNotEmpty()
            )
            val surface = requireNotNull(view.existingTableSurface) {
                "a table must create the table surface"
            }
            val drawing = requireNotNull(tableDrawing(view)) { "the table must be drawn" }
            assertSame(surface.drawingView, drawing)
            assertNotNull(
                "the drawn layout must present the table",
                drawing.preparedLayout?.blocks?.singleOrNull { it.tableSurface != null }
            )

            tapMountedTableCell(view)

            val cellInput = view.activeTextInput
            assertTrue("tapping a cell must activate its input", cellInput !== view.editorEditText)
            assertSame(surface.activeInput, cellInput)
            assertTrue("the cell input must hold focus", cellInput.hasFocus())
        }

    private fun tableDrawing(view: RichTextEditorView): PreparedProseDrawingView? =
        (0 until view.editorContentFrame.childCount)
            .map { view.editorContentFrame.getChildAt(it) }
            .filterIsInstance<PreparedProseDrawingView>()
            .singleOrNull()

    private fun tapAt(view: RichTextEditorView, x: Float, y: Float) {
        val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
        val up = MotionEvent.obtain(0, TAP_DURATION_MS, MotionEvent.ACTION_UP, x, y, 0)
        try {
            view.dispatchTouchEvent(down)
            view.dispatchTouchEvent(up)
        } finally {
            down.recycle()
            up.recycle()
        }
    }

    private companion object {
        const val PLAIN_TEXT = "Plain text"
        const val PLAIN_DOCUMENT = """{"type":"doc","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"$PLAIN_TEXT"}]}]}"""
        const val INSERTED_CELL_TEXT = "Inserted cell"
        const val TABLE_FRAGMENT = """{"type":"doc","content":[{"type":"table",""" +
            """"content":[{"type":"table_row","content":[{"type":"table_cell",""" +
            """"content":[{"type":"paragraph","content":[{"type":"text",""" +
            """"text":"$INSERTED_CELL_TEXT"}]}]}]}]}]}"""
        const val TYPED_TEXT = "!"
        const val SELECTED_LENGTH = 3
        const val TAP_INSET = 12f
        const val TAP_DURATION_MS = 16L
        const val EDITOR_WIDTH = 600
        const val EDITOR_HEIGHT = 500
        const val REMOTE_CLIENT_ID = "7"
        const val REMOTE_NAME = "Remote"
        const val REMOTE_CARET = 2
    }
}
