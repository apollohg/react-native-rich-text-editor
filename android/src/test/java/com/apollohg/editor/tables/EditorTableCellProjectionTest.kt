package com.apollohg.editor.tables

import android.graphics.Color
import android.graphics.Typeface
import android.text.style.StyleSpan
import com.apollohg.editor.EditorTheme
import com.apollohg.editor.TableInputBlock
import com.apollohg.editor.TableInputCell
import com.apollohg.editor.TableInputExcluded
import com.apollohg.editor.TableInputTable
import com.apollohg.editor.parseTableInputMappings
import org.json.JSONArray
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
internal class EditorTableCellProjectionTest {
    private fun table(elements: JSONArray, sourcePos: Int = 2, nested: Boolean = false): JSONObject {
        val key = "a".repeat(64)
        return JSONObject().put("tablePos", 0).put("sourceEnd", 32).put("rows", 1)
            .put("columns", 1).put("columnWidths", JSONArray().put(JSONObject.NULL))
            .put("direction", JSONObject.NULL).put("irregular", false)
            .put("readOnlyDescendants", nested).put("attrsKey", key)
            .put("sourceRows", JSONArray().put(JSONObject().put("sourcePos", 1).put("sourceEnd", 31).put("attrsKey", key)))
            .put("syntheticRegions", JSONArray()).put("failure", JSONObject.NULL)
            .put("compatibilityDiagnostic", JSONObject.NULL)
            .put("cells", JSONArray().put(JSONObject().put("sourcePos", sourcePos)
                .put("sourceEnd", 30).put("row", 0).put("column", 0)
                .put("rowspan", 1).put("colspan", 1).put("header", false)
                .put("attrsKey", key).put("contentKey", "cell").put("elements", elements)))
    }

    private fun block(index: Int, start: Int, contentStart: Int, end: Int,
                      breakEnd: Int = end, isVoid: Boolean = false): TableInputBlock {
        val docStart = 4 + index * 2
        return TableInputBlock(index, docStart, if (isVoid) docStart else docStart + 1,
            start, contentStart, end, breakEnd, isVoid)
    }

    private fun mapping(vararg blocks: TableInputBlock, excluded: Boolean = false) =
        TableInputTable(null, listOf(TableInputCell(
            0, 2, 30, blocks.toList(),
            if (excluded) listOf(TableInputExcluded(0, "nested", null)) else emptyList()
        )))

    private fun admitted(table: JSONObject, vararg blocks: TableInputBlock): TableInputTable {
        val extent = JSONObject().put("scalarStart", blocks.first().scalarStart)
            .put("scalarEnd", blocks.last().scalarEnd)
        val rawBlocks = JSONArray()
        blocks.forEach { block ->
            rawBlocks.put(JSONObject().put("elementIndex", block.elementIndex)
                .put("docStart", block.docStart).put("docEnd", block.docEnd)
                .put("scalarStart", block.scalarStart)
                .put("contentScalarStart", block.contentScalarStart)
                .put("scalarEnd", block.scalarEnd)
                .put("breakScalarEnd", block.breakScalarEnd).put("void", block.isVoid))
        }
        val sidecar = JSONObject().put("extent", extent).put("cells", JSONArray().put(
            JSONObject().put("cellIndex", 0).put("sourcePos", 2).put("sourceEnd", 30)
                .put("blocks", rawBlocks).put("excluded", JSONArray())
        ))
        val parsed = parseTableInputMappings(
            JSONObject().put("version", 1).put("tables", JSONObject().put("t0", sidecar)),
            mapOf("a".repeat(64) to JSONObject()), mapOf("t0" to table),
            blocks.last().scalarEnd
        )
        return requireNotNull(parsed?.tables?.get("t0")) { "rejected ${table.getJSONArray("cells").getJSONObject(0).getJSONArray("elements")}" }
    }

    private fun project(table: JSONObject, mapping: TableInputTable) =
        EditorTableCellProjection.project(0, table, mapping, "4", "9", 16f, Color.BLACK)

    @Test
    fun `rendered rich emoji and paragraph breaks map through final caret`() {
        val elements = JSONArray("""[
            {"type":"blockStart","nodeType":"paragraph","depth":0},
            {"type":"textRun","text":"a😀","marks":["bold"]},
            {"type":"blockEnd"},
            {"type":"blockStart","nodeType":"paragraph","depth":0},
            {"type":"textRun","text":"b","marks":[]},
            {"type":"blockEnd"}
        ]""")
        val record = table(elements)
        val projection = requireNotNull(project(record, admitted(record,
            block(0, 40, 40, 42, 43), block(3, 43, 43, 44)
        )))

        assertEquals("a😀\nb", projection.text.toString())
        assertTrue(projection.text.getSpans(0, 3, StyleSpan::class.java).any { it.style == Typeface.BOLD })
        assertEquals(42, projection.positionMap.globalScalarForLocalUtf16(3, projection.text.toString()))
        assertEquals(43, projection.positionMap.globalScalarForLocalUtf16(4, projection.text.toString()))
        assertEquals(44, projection.positionMap.globalScalarForLocalUtf16(5, projection.text.toString()))
        assertEquals(TableCellPositionMap.ScalarRange(40, 44), projection.positionMap.globalScalarRange(0, 4))
        assertEquals(2L, projection.target.binding.cellSourcePos)
    }

    @Test
    fun `empty paragraph has exactly one mapped local scalar`() {
        val elements = JSONArray("""[
            {"type":"blockStart","nodeType":"paragraph","depth":0},
            {"type":"blockEnd"}
        ]""")
        val record = table(elements)
        val projection = requireNotNull(project(record, admitted(record, block(0, 7, 7, 8))))

        assertEquals(1, projection.text.toString().codePointCount(0, projection.text.length))
        assertEquals(7, projection.positionMap.globalScalarForLocalScalar(0))
        assertEquals(8, projection.positionMap.globalScalarForLocalScalar(1))
        assertNull(projection.positionMap.globalScalarForLocalScalar(2))
    }

    @Test
    fun `empty first paragraph and next paragraph keep the rendered break mapped`() {
        val elements = JSONArray("""[
            {"type":"blockStart","nodeType":"paragraph","depth":0},
            {"type":"blockEnd"},
            {"type":"blockStart","nodeType":"paragraph","depth":0},
            {"type":"textRun","text":"two","marks":[]},
            {"type":"blockEnd"}
        ]""")
        val record = table(elements)
        val first = block(0, 11, 11, 12, 13)
        val second = block(2, 13, 13, 16)
        val projection = requireNotNull(project(record, admitted(record, first, second)))

        assertEquals("\u200B\ntwo", projection.text.toString())
        assertEquals(11, projection.positionMap.globalScalarForLocalUtf16(0, projection.text.toString()))
        assertEquals(12, projection.positionMap.globalScalarForLocalUtf16(1, projection.text.toString()))
        assertEquals(13, projection.positionMap.globalScalarForLocalUtf16(2, projection.text.toString()))
        assertEquals(16, projection.positionMap.globalScalarForLocalUtf16(5, projection.text.toString()))
        assertEquals(TableCellPositionMap.ScalarRange(11, 16), projection.positionMap.globalScalarRange(0, 5))
    }

    @Test
    fun `projection rejects admitted global gaps and false break metadata`() {
        val elements = JSONArray("""[
            {"type":"blockStart","nodeType":"paragraph","depth":0},
            {"type":"textRun","text":"a","marks":[]},
            {"type":"blockEnd"},
            {"type":"blockStart","nodeType":"paragraph","depth":0},
            {"type":"textRun","text":"b","marks":[]},
            {"type":"blockEnd"}
        ]""")
        val record = table(elements)

        val gap = admitted(record, block(0, 40, 40, 41, 42), block(3, 100, 100, 101))
        assertNull(project(record, gap))

        val falseBreak = admitted(record, block(0, 40, 40, 41), block(3, 41, 41, 42))
        assertNull(project(record, falseBreak))
    }

    @Test
    fun `list prefix and void atom use emitted renderer ranges`() {
        val listElements = JSONArray("""[
            {"type":"blockStart","nodeType":"listItem","depth":0,"listContext":{"ordered":true,"index":1,"total":1,"start":1,"isFirst":true,"isLast":true}},
            {"type":"blockStart","nodeType":"paragraph","depth":1},
            {"type":"textRun","text":"x","marks":[]},
            {"type":"blockEnd"},{"type":"blockEnd"}
        ]""")
        val listRecord = table(listElements)
        val list = requireNotNull(project(listRecord, admitted(listRecord, block(1, 20, 23, 24))))
        assertEquals("1. x", list.text.toString())
        assertEquals(20, list.positionMap.globalScalarForLocalScalar(0))
        assertEquals(23, list.positionMap.globalScalarForLocalScalar(3))
        assertEquals(24, list.positionMap.globalScalarForLocalScalar(4))
        val theme = EditorTheme.fromJson("""{"version":1,"styles":{"paragraph":{"paddingLeft":10}}}""")
        val styled = requireNotNull(EditorTableCellProjection.project(
            0, listRecord, admitted(listRecord, block(1, 20, 23, 24)), "4", "9",
            16f, Color.BLACK, theme
        ))
        assertEquals(list.text.toString(), styled.text.toString())
        assertEquals(24, styled.positionMap.globalScalarForLocalScalar(4))

        val atomElements = JSONArray("""[{"type":"voidBlock","nodeType":"rule","docPos":4}]""")
        val atomRecord = table(atomElements)
        val atom = requireNotNull(project(atomRecord, admitted(atomRecord, block(0, 30, 30, 31, isVoid = true))))
        assertEquals("\uFFFC", atom.text.toString())
        assertEquals(31, atom.positionMap.globalScalarForLocalScalar(1))
    }

    @Test
    fun `projection rejects mismatches and unsafe cell associations`() {
        val elements = JSONArray("""[
            {"type":"blockStart","nodeType":"paragraph","depth":0},
            {"type":"textRun","text":"x","marks":[]},
            {"type":"blockEnd"}
        ]""")
        val record = table(elements)
        val valid = admitted(record, block(0, 4, 4, 5))
        assertNotNull(project(record, valid))
        assertNull(project(record, mapping(block(0, 4, 4, 6))))
        assertNull(project(record, mapping(block(0, Int.MAX_VALUE, Int.MAX_VALUE, Int.MAX_VALUE))))
        assertNull(project(record, mapping(block(0, 4, 4, 5), excluded = true)))
        assertNull(project(table(elements, nested = true), valid))
        assertNull(project(table(elements, sourcePos = 3), valid))
        assertNull(EditorTableCellProjection.project(1, record, valid, "4", "9", 16f, Color.BLACK))

        val extra = JSONArray("""[
            {"type":"blockStart","nodeType":"paragraph","depth":0},
            {"type":"textRun","text":"x","marks":[]},
            {"type":"blockEnd"},
            {"type":"blockStart","nodeType":"paragraph","depth":0},
            {"type":"textRun","text":"y","marks":[]},
            {"type":"blockEnd"}
        ]""")
        assertNull(project(table(extra), valid))
    }

    @Test
    fun `projection fails closed when hard break adds an unmapped renderer placeholder`() {
        val elements = JSONArray("""[
            {"type":"blockStart","nodeType":"paragraph","depth":0},
            {"type":"textRun","text":"x","marks":[]},
            {"type":"voidInline","nodeType":"hardBreak","docPos":5},
            {"type":"blockEnd"}
        ]""")
        val record = table(elements)
        assertNull(project(record, admitted(record, block(0, 8, 8, 10))))
    }
}
