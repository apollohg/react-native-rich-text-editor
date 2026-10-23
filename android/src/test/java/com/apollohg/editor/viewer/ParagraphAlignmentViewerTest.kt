package com.apollohg.editor.viewer

import android.text.Layout.Alignment
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class ParagraphAlignmentViewerTest {
    @Test
    fun `legacy viewer theme retains physical paragraph alignment`() {
        val theme = PreparedProseTheme.resolve("""{"paragraph":{"textAlign":"right"}}""", 1f)
        val block = ViewerBlock("paragraph", 0, false, null, null, emptyList())
        assertEquals("right", theme.paintFor(block).textAlign)
    }

    @Test
    fun `cell lowering and detached snapshots preserve explicit alignment over theme`() {
        val theme = PreparedProseTheme.resolve(
            """{"version":1,"styles":{"paragraph":{"textAlign":"center"}}}""",
            1f
        )
        val parent = ViewerDocument("parent", emptyList(), false, 0)
        for (align in listOf("left", "center", "right", null)) {
            val elements = mutableListOf<uniffi.editor_core.FfiViewerElement>(
                uniffi.editor_core.FfiViewerElement.BlockStart("paragraph", null, 0u, null, align),
                uniffi.editor_core.FfiViewerElement.TextRun("العربية", emptyList()),
                uniffi.editor_core.FfiViewerElement.BlockEnd
            )
            val cell = com.apollohg.editor.tables.TableSurfaceCell(
                0, 0, 0, 1, 1, false, "attrs", "content-$align", elements
            )
            val document = parent.cellDocument(cell, "table")
            val block = document.blocks.single()
            assertEquals(align ?: "center", theme.paintFor(block).textAlign)
            assertEquals(align, PreparedProseTheme.resolve(null, 1f).paintFor(block).textAlign)
            val editorElements = requireNotNull(
                com.apollohg.editor.tables.inputElements(
                    elements,
                    emptyList()
                ) { it }
            )
            assertEquals(align, editorElements.getJSONObject(0).opt("textAlign") as? String)
            val editorText = com.apollohg.editor.RenderBridge.buildSpannable(
                editorElements.toString(),
                16f,
                android.graphics.Color.BLACK,
                com.apollohg.editor.EditorTheme.fromJson(
                    """{"version":1,"styles":{"paragraph":{"textAlign":"center"}}}"""
                )
            )
            assertEquals(
                listOf(align ?: "center"),
                editorText.getSpans(
                    0,
                    editorText.length,
                    com.apollohg.editor.EditorParagraphAlignmentSpan::class.java
                ).map { it.value }
            )
            val detached = requireNotNull(PlainTableCellDocument.capture(parent, cell, "table"))
            elements.clear()
            assertEquals(document, detached.materialize())
            assertEquals(
                align ?: "center",
                theme.paintFor(detached.materialize().blocks.single()).textAlign
            )
        }
    }

    @Test
    fun `empty and RTL viewer blocks use physical alignment and reset returns theme`() {
        val theme = PreparedProseTheme.resolve(
            """{"version":1,"styles":{"paragraph":{"textAlign":"center"}}}""",
            1f
        )
        for (text in listOf("", "العربية")) {
            for (align in listOf("left", "center", "right", null)) {
                val block = ViewerBlock(
                    "paragraph",
                    0,
                    false,
                    null,
                    null,
                    listOf(ViewerInline.Text(text, emptyList())),
                    textAlign = align
                )
                val document = ViewerDocument("document-$text-$align", listOf(block), false, 0)
                val prepared = StaticLayoutAndroidProseLayoutEngine().prepare(
                    document,
                    ProseLayoutKey(document.semanticKey, 300, "theme", 0, 0, 0, 0, "test"),
                    theme,
                    300,
                    1f,
                    false
                )
                val layout = requireNotNull(
                    prepared.blocks.single().fragments.first {
                        it.layout !=
                            null
                    }.layout
                )
                val expected = when (align) {
                    "left" -> if (text.isEmpty()) {
                        Alignment.ALIGN_NORMAL
                    } else {
                        Alignment.ALIGN_OPPOSITE
                    }

                    "right" -> if (text.isEmpty()) {
                        Alignment.ALIGN_OPPOSITE
                    } else {
                        Alignment.ALIGN_NORMAL
                    }

                    else -> Alignment.ALIGN_CENTER
                }
                assertEquals("text=$text align=$align", expected, layout.alignment)
            }
        }
    }
}
