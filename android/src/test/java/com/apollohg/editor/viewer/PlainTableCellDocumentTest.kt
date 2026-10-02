package com.apollohg.editor.viewer

import com.apollohg.editor.tables.EditorTableIndex
import com.apollohg.editor.tables.TableSurfaceCell
import java.lang.ref.WeakReference
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.editor_core.FfiViewerElement
import uniffi.editor_core.FfiViewerMark

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class PlainTableCellDocumentTest {
    private fun cell(elements: List<FfiViewerElement>) =
        TableSurfaceCell(0, 0, 0, 1, 1, false, "attrs", "content", elements)

    @Test
    fun detachedPlainCellsMaterializeTheExactOrdinaryDocumentAfterSourceMutation() {
        for (text in listOf(null, "", "plain", "a😀e\u0301 العربية\nnext")) {
            for (depth in listOf<UShort>(0u, 7u)) {
                val elements = mutableListOf<FfiViewerElement>(
                    FfiViewerElement.BlockStart("paragraph", "language", depth, null))
                val marks = mutableListOf<FfiViewerMark>()
                if (text != null) elements += FfiViewerElement.TextRun(text, marks)
                elements += FfiViewerElement.BlockEnd
                val parent = ViewerDocument("parent", emptyList(), false, 0, preferredTextBlockName = "customParagraph",
                    frameIndex = EditorTableIndex())
                val source = cell(elements)
                val expected = parent.cellDocument(cell(elements.map { element ->
                    if (element is FfiViewerElement.TextRun) element.copy(marks = element.marks.toList()) else element
                }), TABLE_KEY)
                val captured = requireNotNull(PlainTableCellDocument.capture(parent, source, TABLE_KEY))
                elements.clear()
                marks += FfiViewerMark("bold", "{}")
                assertEquals("Original text must not contain measurement placeholders", text.orEmpty(), captured.text)
                assertEquals("The child preserves key, depth, language, empty-run presence and preferred block", expected, captured.materialize())
                assertEquals(expected, captured.materialize())
            }
        }
    }

    @Test
    fun richNestedMalformedAndListSourcesKeepTheOrdinaryConversion() {
        val start = FfiViewerElement.BlockStart("paragraph", null, 0u, null)
        val text = FfiViewerElement.TextRun("text", emptyList())
        val end = FfiViewerElement.BlockEnd
        val parent = ViewerDocument("parent", emptyList(), false, 0)
        val cases = mapOf(
            "empty sequence" to emptyList(),
            "unclosed" to listOf(start, text),
            "stray end" to listOf(end, text, end),
            "two runs" to listOf(start, text, text, end),
            "marked" to listOf(start, text.copy(marks = listOf(FfiViewerMark("bold", "{}"))), end),
            "list" to listOf(start.copy(listContextJson = "{}"), text, end),
            "heading" to listOf(start.copy(nodeType = "heading"), text, end),
            "nested" to listOf(start, FfiViewerElement.Table("nested"), end),
            "container" to listOf(start.copy(nodeType = "blockquote"), start, text, end, end)
        )
        cases.forEach { (label, elements) ->
            assertNull(label, PlainTableCellDocument.capture(parent, cell(elements), TABLE_KEY))
        }
    }

    @Test
    fun retainedPlainSourceDoesNotKeepTheParentDocumentOrFrameIndexAlive() {
        val references = mutableListOf<WeakReference<*>>()
        fun capture(): PlainTableCellDocument {
            val index = EditorTableIndex()
            val parent = ViewerDocument("parent", emptyList(), false, 0, frameIndex = index)
            val elements = mutableListOf<FfiViewerElement>(FfiViewerElement.BlockStart("paragraph", null, 0u, null),
                FfiViewerElement.TextRun("retained", emptyList()), FfiViewerElement.BlockEnd)
            references += WeakReference(parent)
            references += WeakReference(index)
            references += WeakReference(elements)
            return requireNotNull(PlainTableCellDocument.capture(parent, cell(elements), TABLE_KEY))
        }
        val retained = capture()
        repeat(GC_ATTEMPTS) { System.gc(); System.runFinalization() }
        assertTrue("A retained refill source must release its parent, index and mutable source list", references.all { it.get() == null })
        assertEquals("retained", (retained.materialize().blocks.single().inlines.single() as ViewerInline.Text).text)
    }

    companion object {
        private const val TABLE_KEY = "table"
        private const val GC_ATTEMPTS = 8
    }
}
