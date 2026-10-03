package com.apollohg.editor

import android.app.Activity
import android.graphics.Rect
import android.os.Looper
import android.view.View
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityNodeProvider
import android.widget.FrameLayout
import com.apollohg.editor.tables.TableAccessibilityAction
import com.apollohg.editor.tables.TableAccessibilityDetachedFrame
import com.apollohg.editor.tables.TableAccessibilityEditing
import com.apollohg.editor.tables.TableAccessibilityNodes
import com.apollohg.editor.tables.TableCellAccessibility
import java.io.File
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], qualifiers = "w960dp-h640dp")
internal class EditorTableAccessibilityTest {
    private class RecordingBackend : EditorV2Backend by UniffiEditorV2Backend {
        val commands = mutableListOf<JSONObject>()

        override fun applyCommand(
            editorId: String,
            requestJson: String
        ): EditorV2CallResult<String> {
            commands += JSONObject(requestJson).getJSONObject("command")
            return UniffiEditorV2Backend.applyCommand(editorId, requestJson)
        }
    }

    private class Fixture(
        val view: RichTextEditorView,
        val adapter: EditorV2Adapter,
        val backend: RecordingBackend,
        val updates: MutableList<JSONObject>
    ) {
        val root: EditorEditText get() = view.editorEditText
        val provider: AccessibilityNodeProvider
            get() = view.editorTableSurface.drawingView.accessibilityNodeProvider

        fun tableNodes(): List<Pair<Int, AccessibilityNodeInfo>> =
            generateSequence(TableAccessibilityNodes.FIRST_TABLE_NODE_ID) { it + 1 }
                .map { id -> provider.createAccessibilityNodeInfo(id)?.let { id to it } }
                .takeWhile { it != null }.filterNotNull().toList()

        fun cellNodes(): List<Pair<Int, AccessibilityNodeInfo>> =
            tableNodes().filter { (_, info) -> info.collectionItemInfo != null }

        fun frameNode(): Pair<Int, AccessibilityNodeInfo> = tableNodes().single { (_, info) ->
            info.text?.toString() == RuntimeEnvironment.getApplication()
                .getString(R.string.table_accessibility_empty_table)
        }

        fun openings(): List<Int> {
            val table = adapter.tableRecordsForTesting.values.filter {
                it.getJSONArray("cells").length() >
                    0
            }
                .minBy { it.getInt("tablePos") }
            val cells = table.getJSONArray("cells")
            return (0 until cells.length()).map { cells.getJSONObject(it).getInt("sourcePos") }
        }

        fun selectCells(anchor: Int, head: Int) {
            fun point(opening: Int) = JSONObject().put("kind", "document").put("offset", opening)
            val selection = JSONObject().put("type", CELL_SELECTION)
                .put("anchorCell", point(anchor)).put("headCell", point(head))
            val admitted = adapter.callWithEnvelope(JSONObject().put("selection", selection)) {
                UniffiEditorV2Backend.setSelection(adapter.editorId, it)
            }
            assertTrue(
                "engine rejected the selection: $admitted",
                admitted is EditorV2CallResult.Ok
            )
            assertTrue(root.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
            relayout()
            backend.commands.clear()
            updates.clear()
        }

        fun relayout() {
            view.measure(
                View.MeasureSpec.makeMeasureSpec(VIEW_WIDTH, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(VIEW_HEIGHT, View.MeasureSpec.EXACTLY)
            )
            view.layout(0, 0, VIEW_WIDTH, VIEW_HEIGHT)
            val drawing = view.editorTableSurface.drawingView
            drawing.measure(
                View.MeasureSpec.makeMeasureSpec(root.width, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(root.height, View.MeasureSpec.EXACTLY)
            )
            drawing.layout(0, 0, drawing.measuredWidth, drawing.measuredHeight)
            shadowOf(Looper.getMainLooper()).idle()
        }

        fun publishedActionIds(): List<Int> {
            val commands = requireNotNull(adapter.cachedActiveState).getJSONObject("commands")
            return TableAccessibilityAction.ALL.filter {
                commands.optBoolean(it.applicability, false)
            }.map { it.id }
        }

        fun blockTypes(): List<String> = JSONObject(requireNotNull(adapter.documentJson()))
            .getJSONArray("content").let { content ->
                (0 until content.length()).map { content.getJSONObject(it).getString("type") }
            }

        fun rowCount(): Int = JSONObject(
            requireNotNull(adapter.documentJson())
        ).getJSONArray("content").let { content ->
            (0 until content.length()).map(content::getJSONObject).first {
                it.getString("type") ==
                    TABLE_NODE
            }
                .getJSONArray("content").length()
        }

        fun assertOneUndoableCommand(command: String, before: String?) {
            assertEquals(
                "exactly one engine command",
                listOf(command),
                backend.commands.map {
                    it.getString("type")
                }
            )
            assertEquals("exactly one published update", 1, updates.size)
            assertTrue(requireNotNull(adapter.historyCanUndo()))
            assertTrue(root.applyUpdateJSON(requireNotNull(adapter.undo())))
            assertEquals("one undo restores the document", before, adapter.documentJson())
            assertFalse(
                "the action was one history entry",
                requireNotNull(adapter.historyCanUndo())
            )
        }
    }

    private fun customActionIds(info: AccessibilityNodeInfo): List<Int> = info.actionList.map {
        it.id
    }.filter { id -> TableAccessibilityAction.ALL.any { it.id == id } }

    private fun withTable(document: String, block: (Fixture) -> Unit) {
        val created = UniffiEditorV2Backend.create(TABLE_CONFIG, null) as EditorV2CallResult.Ok
        val backend = RecordingBackend()
        val adapter = requireNotNull(
            EditorV2Adapter.attach(backend, JSONObject(created.value).getString("editorId"), false)
        )
        val token = EditorV2Registry.register(adapter)
        val activity = Robolectric.buildActivity(Activity::class.java)
        try {
            val view = RichTextEditorView(activity.create().get())
            activity.get().setContentView(
                FrameLayout(activity.get()).apply {
                    addView(view, FrameLayout.LayoutParams(VIEW_WIDTH, VIEW_HEIGHT))
                }
            )
            activity.start().resume().visible()
            view.editorId = token
            assertTrue(
                view.editorEditText.applyUpdateJSON(
                    requireNotNull(adapter.setContentJson(document))
                )
            )
            val updates = mutableListOf<JSONObject>()
            val fixture = Fixture(view, adapter, backend, updates)
            fixture.relayout()
            view.editorEditText.requestFocus()
            view.editorEditText.editorListener = object : EditorEditText.EditorListener {
                override fun onEditorUpdate(updateJSON: String) {
                    updates += JSONObject(updateJSON)
                }
                override fun onSelectionChanged(anchor: Int, head: Int) = Unit
            }
            assertFalse(
                "fixture must start without history",
                requireNotNull(adapter.historyCanUndo())
            )
            backend.commands.clear()
            block(fixture)
        } finally {
            activity.pause().stop().destroy()
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `editor table exposes a collection whose cells follow document order`() =
        withTable(FOUR_CELL_DOCUMENT) { fixture ->
            val table = fixture.tableNodes().first().second
            assertEquals(2, table.collectionInfo.rowCount)
            assertEquals(2, table.collectionInfo.columnCount)
            val cells = fixture.cellNodes().map { it.second }
            assertEquals(listOf("one", "two", "three", "four"), cells.map { it.text.toString() })
            assertEquals(
                listOf(0 to 0, 0 to 1, 1 to 0, 1 to 1),
                cells.map { it.collectionItemInfo.rowIndex to it.collectionItemInfo.columnIndex }
            )
            assertTrue(
                "the editor surface publishes its table subtree",
                fixture.view.editorTableSurface.drawingView.isImportantForAccessibility
            )
        }

    @Test
    fun `native table actions match the toolbar action fixture`() {
        val workingDirectory = requireNotNull(System.getProperty("user.dir"))
        val fixtureFile = generateSequence(File(workingDirectory)) { it.parentFile }
            .map { File(it, PARITY_FIXTURE) }.first { it.isFile }
        val fixture = JSONArray(fixtureFile.readText())
        assertEquals(
            "native actions cover every toolbar action",
            TableAccessibilityAction.ALL.size,
            fixture.length()
        )
        val resources = RuntimeEnvironment.getApplication().resources
        TableAccessibilityAction.ALL.forEachIndexed { index, action ->
            val expected = fixture.getJSONObject(index)
            val name = expected.getString("action")
            val command = expected.getJSONObject("command")
            assertEquals(name, expected.getString("applicability"), action.applicability)
            assertEquals(
                name,
                command.keys().asSequence().associateWith(command::getString),
                action.command
            )
            val resourceName =
                RESOURCE_PREFIX + name.replace(Regex("([A-Z])")) { "_" + it.value.lowercase() }
            assertEquals(name, resourceName, resources.getResourceEntryName(action.id))
            assertEquals(name, resourceName, resources.getResourceEntryName(action.label))
        }
    }

    @Test
    fun `frames are ordered among drawn tables by document position`() =
        withTable(FRAME_BETWEEN_TABLES_DOCUMENT) { fixture ->
            val drawing = fixture.view.editorTableSurface.drawingView
            val children = drawing.accessibilityChildIds().map {
                requireNotNull(fixture.provider.createAccessibilityNodeInfo(it))
            }
            val app = RuntimeEnvironment.getApplication()
            assertEquals(
                listOf(
                    app.getString(R.string.table_accessibility_table),
                    app.getString(R.string.table_accessibility_empty_table),
                    app.getString(R.string.table_accessibility_table)
                ),
                children.map { (it.contentDescription ?: it.text).toString() }
            )
            val tops = children.map { Rect().also(it::getBoundsInParent).top }
            assertEquals("children follow document position: $tops", tops.sorted(), tops)
        }

    @Test
    fun `table accessibility nodes reuse one snapshot until the presentation changes`() =
        withTable(FRAME_BESIDE_TABLE_DOCUMENT) { fixture ->
            val surface = fixture.view.editorTableSurface
            val drawing = surface.drawingView
            var frameQueries = 0
            drawing.tableAccessibilityEditing = object : TableAccessibilityEditing by surface {
                override fun detachedTableAccessibilityFrames() =
                    surface.detachedTableAccessibilityFrames().also { frameQueries += 1 }
            }
            repeat(SNAPSHOT_READS) { fixture.tableNodes() }
            assertEquals("repeated node reads share one snapshot", 1, frameQueries)

            assertTrue(
                fixture.root.applyUpdateJSON(
                    requireNotNull(fixture.adapter.setContentJson(FOUR_CELL_DOCUMENT))
                )
            )
            fixture.relayout()
            val texts = fixture.cellNodes().map { it.second.text.toString() }
            assertEquals(
                "a document change rebuilds the snapshot",
                listOf("one", "two", "three", "four"),
                texts
            )
            assertEquals(2, frameQueries)
        }

    @Test
    fun `a detached frame follows its line without a presentation change`() =
        withTable(EMPTY_FRAME_DOCUMENT) { fixture ->
            val before = Rect().also(fixture.frameNode().second::getBoundsInParent)
            val generation =
                fixture.view.editorTableSurface.drawingView.tableAccessibilityGeneration
            fixture.root.setPadding(0, FRAME_SHIFT_PX, 0, 0)
            fixture.relayout()
            assertEquals(
                "padding alone must not rebuild the snapshot",
                generation,
                fixture.view.editorTableSurface.drawingView.tableAccessibilityGeneration
            )
            val after = Rect().also(fixture.frameNode().second::getBoundsInParent)
            assertEquals(
                "the frame is placed from the current line",
                before.top + FRAME_SHIFT_PX,
                after.top
            )
        }

    @Test
    fun `a detached frame node places itself with one bounds evaluation`() =
        withTable(EMPTY_FRAME_DOCUMENT) { fixture ->
            val surface = fixture.view.editorTableSurface
            var evaluations = 0
            surface.drawingView.tableAccessibilityEditing =
                object : TableAccessibilityEditing by surface {
                    override fun detachedTableAccessibilityFrames() =
                        surface.detachedTableAccessibilityFrames().map { frame ->
                            frame.copy(bounds = {
                                evaluations += 1
                                frame.bounds()
                            })
                        }
                }
            val (frameId) = fixture.frameNode()
            evaluations = 0
            assertNotNull(fixture.provider.createAccessibilityNodeInfo(frameId))
            assertEquals("one node build measures its line once", 1, evaluations)
        }

    @Test
    fun `an input without a table slot stays in the editor frame traversal`() =
        withTable(FOUR_CELL_DOCUMENT) { fixture ->
            val surface = fixture.view.editorTableSurface
            val orphan = EditorEditText(fixture.view.context)
            orphan.tableCellAccessibility =
                TableCellAccessibility(surface.drawingView, { null }, FIRST_CELL, surface)
            fixture.view.editorContentFrame.addView(orphan)
            val children = ArrayList<View>().also(
                fixture.view.editorContentFrame::addChildrenForAccessibility
            )
            assertTrue("an input with no virtual parent must not be orphaned", orphan in children)
        }

    @Test
    fun `selected cells expose exactly the published table actions`() =
        withTable(FOUR_CELL_DOCUMENT) { fixture ->
            val openings = fixture.openings()
            fixture.selectCells(openings[FIRST_CELL], openings[SECOND_CELL])
            val expected = fixture.publishedActionIds()
            val merge = TableAccessibilityAction.ALL.single {
                it.id ==
                    R.id.table_accessibility_merge_cells
            }
            val split = TableAccessibilityAction.ALL.single {
                it.id ==
                    R.id.table_accessibility_split_cell
            }
            assertTrue("a two-cell selection must publish merge: $expected", merge.id in expected)
            assertFalse(split.id in expected)
            val cells = fixture.cellNodes().map { it.second }
            assertEquals(expected, customActionIds(cells[FIRST_CELL]))
            assertEquals(expected, customActionIds(cells[SECOND_CELL]))
            assertEquals(
                "unselected cells carry no table actions",
                emptyList<Int>(),
                customActionIds(cells[THIRD_CELL])
            )
            val label = cells[FIRST_CELL].actionList.single { it.id == merge.id }.label.toString()
            assertEquals(
                RuntimeEnvironment.getApplication().getString(
                    R.string.table_accessibility_merge_cells
                ),
                label
            )
        }

    @Test
    fun `an accessibility row action performs exactly one command`() =
        withTable(FOUR_CELL_DOCUMENT) { fixture ->
            val openings = fixture.openings()
            fixture.selectCells(openings[FIRST_CELL], openings[FIRST_CELL])
            val before = fixture.adapter.documentJson()
            val insertBelow = TableAccessibilityAction.ALL.single {
                it.id ==
                    R.id.table_accessibility_add_row_after
            }
            val (cellId) = fixture.cellNodes()[FIRST_CELL]

            assertTrue(fixture.provider.performAction(cellId, insertBelow.id, null))

            assertEquals(3, fixture.rowCount())
            fixture.assertOneUndoableCommand(ADD_TABLE_ROW, before)
        }

    @Test
    fun `an empty frame beside a table deletes exactly that table in one mutation`() =
        withTable(FRAME_BESIDE_TABLE_DOCUMENT) { fixture ->
            val before = fixture.adapter.documentJson()
            val frameTablePos = fixture.adapter.tableRecordsForTesting.values
                .single { it.getJSONArray("cells").length() == 0 }.getInt("tablePos")
            val (frameId, frame) = fixture.frameNode()
            val bounds = android.graphics.Rect().also(frame::getBoundsInParent)
            assertFalse("the frame is placed at its document line: $bounds", bounds.isEmpty)
            assertTrue(
                "the frame lies inside the editor: $bounds",
                android.graphics.Rect(
                    0,
                    0,
                    fixture.root.width,
                    fixture.root.height
                ).contains(bounds)
            )
            assertEquals(listOf(TableAccessibilityAction.DELETE_TABLE.id), customActionIds(frame))

            assertTrue(
                fixture.provider.performAction(
                    frameId,
                    TableAccessibilityAction.DELETE_TABLE.id,
                    null
                )
            )

            assertEquals(listOf(PARAGRAPH_NODE, TABLE_NODE, PARAGRAPH_NODE), fixture.blockTypes())
            assertEquals("the neighbouring table survives", 1, fixture.rowCount())
            assertEquals(
                "the delete targets the frame",
                frameTablePos,
                fixture.backend.commands.single().getInt("tablePos")
            )
            fixture.assertOneUndoableCommand(DELETE_TABLE, before)
        }

    @Test
    fun `an editor holding only an empty frame still exposes its delete`() =
        withTable(EMPTY_FRAME_DOCUMENT) { fixture ->
            val drawing = fixture.view.editorTableSurface.drawingView
            assertSame(
                "the frame's accessibility host stays mounted",
                fixture.view.editorContentFrame,
                drawing.parent
            )
            val before = fixture.adapter.documentJson()
            val (frameId, frame) = fixture.frameNode()
            assertEquals(listOf(TableAccessibilityAction.DELETE_TABLE.id), customActionIds(frame))
            assertTrue(
                fixture.provider.performAction(
                    frameId,
                    TableAccessibilityAction.DELETE_TABLE.id,
                    null
                )
            )
            assertEquals(listOf(PARAGRAPH_NODE, PARAGRAPH_NODE), fixture.blockTypes())
            fixture.assertOneUndoableCommand(DELETE_TABLE, before)
        }

    @Test
    fun `read only editor exposes no table actions or frame delete`() =
        withTable(FRAME_BESIDE_TABLE_DOCUMENT) { fixture ->
            val openings = fixture.openings()
            fixture.selectCells(openings[FIRST_CELL], openings[FIRST_CELL])
            fixture.root.isEditable = false
            val before = fixture.adapter.documentJson()
            val (cellId, cell) = fixture.cellNodes().single()
            assertEquals(emptyList<Int>(), customActionIds(cell))
            val (frameId, frame) = fixture.frameNode()
            assertEquals(emptyList<Int>(), customActionIds(frame))
            assertFalse(
                fixture.provider.performAction(
                    frameId,
                    TableAccessibilityAction.DELETE_TABLE.id,
                    null
                )
            )
            assertFalse(
                fixture.provider.performAction(
                    cellId,
                    TableAccessibilityAction.ALL.single {
                        it.id ==
                            R.id.table_accessibility_add_row_after
                    }.id,
                    null
                )
            )
            assertEquals(before, fixture.adapter.documentJson())
            assertTrue(fixture.backend.commands.isEmpty())
        }

    @Test
    fun `an activated cell exposes the real input with the cell semantics and actions`() =
        withTable(FOUR_CELL_DOCUMENT) { fixture ->
            val (cellId) = fixture.cellNodes()[LAST_CELL]
            assertTrue(
                fixture.provider.performAction(cellId, AccessibilityNodeInfo.ACTION_CLICK, null)
            )
            fixture.relayout()
            val input = requireNotNull(fixture.view.editorTableSurface.activeInput)
            val frameChildren = ArrayList<View>().also(
                fixture.view.editorContentFrame::addChildrenForAccessibility
            )
            assertFalse(
                "the bound input is reachable only through its grid slot",
                input in frameChildren
            )
            assertTrue("the root editor stays a frame child", fixture.root in frameChildren)

            val info = input.createAccessibilityNodeInfo()
            val item =
                requireNotNull(info.collectionItemInfo) { "the input must carry the cell position" }
            assertEquals(1 to 1, item.rowIndex to item.columnIndex)
            val expected = fixture.publishedActionIds()
            assertTrue(expected.isNotEmpty())
            assertEquals(expected, customActionIds(info))
            val insertBelow = TableAccessibilityAction.ALL.single {
                it.id ==
                    R.id.table_accessibility_add_row_after
            }
            val before = fixture.adapter.documentJson()
            fixture.backend.commands.clear()
            fixture.updates.clear()
            assertTrue(input.performAccessibilityAction(insertBelow.id, null))
            assertEquals(3, fixture.rowCount())
            assertEquals(
                listOf(ADD_TABLE_ROW),
                fixture.backend.commands.map {
                    it.getString("type")
                }
            )
            assertTrue(fixture.root.applyUpdateJSON(requireNotNull(fixture.adapter.undo())))
            assertEquals(before, fixture.adapter.documentJson())
        }

    private companion object {
        const val VIEW_WIDTH = 900
        const val VIEW_HEIGHT = 500
        const val CELL_SELECTION = "cell"
        const val TABLE_NODE = "table"
        const val PARAGRAPH_NODE = "paragraph"
        const val ADD_TABLE_ROW = "addTableRow"
        const val DELETE_TABLE = "deleteTable"
        const val PARITY_FIXTURE = "scripts/tests/table-toolbar-actions.json"
        const val RESOURCE_PREFIX = "table_accessibility_"
        const val SNAPSHOT_READS = 3
        const val FRAME_SHIFT_PX = 40
        const val FIRST_CELL = 0
        const val SECOND_CELL = 1
        const val THIRD_CELL = 2
        const val LAST_CELL = 3
        const val TABLE_CONFIG = """{"schema":{"nodes":[{"name":"doc","content":"block+",""" +
            """"role":"doc"},{"name":"paragraph","content":"inline*",""" +
            """"group":"block","role":"textBlock"},{"name":"text",""" +
            """"content":"","group":"inline","role":"text"},""" +
            """{"name":"table","content":"table_row+","group":"block",""" +
            """"role":"block","tableRole":"table"},""" +
            """{"name":"table_row","content":"(table_cell | """ +
            """table_header)*","role":"block","tableRole":"row"},""" +
            """{"name":"table_cell","content":"block+","role":"block",""" +
            """"tableRole":"cell","attrs":{"colspan":{"type":"number",""" +
            """"default":1,"min":1},"rowspan":{"type":"number",""" +
            """"default":1,"min":1},"colwidth":{"default":null}}},""" +
            """{"name":"table_header","content":"block+",""" +
            """"role":"block","tableRole":"header_cell",""" +
            """"attrs":{"colspan":{"type":"number","default":1,""" +
            """"min":1},"rowspan":{"type":"number","default":1,""" +
            """"min":1},"colwidth":{"default":null}}}],"marks":[]},""" +
            """"initialization":{"type":"localEmpty"}}"""
        const val FOUR_CELL_DOCUMENT = """{"type":"doc","content":[{"type":"table",""" +
            """"content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell",""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"one"}]}]},""" +
            """{"type":"table_cell","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"two"}]}]}]},""" +
            """{"type":"table_row","content":[{"type":"table_cell",""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"three"}]}]},""" +
            """{"type":"table_cell","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"four"}]}]}]}]}]}"""
        const val EMPTY_FRAME_DOCUMENT = """{"type":"doc","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"before"}]},""" +
            """{"type":"table"},{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"after"}]}]}"""
        const val FRAME_BETWEEN_TABLES_DOCUMENT = """{"type":"doc","content":[{"type":"table",""" +
            """"content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell",""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"first"}]}]}]}]},""" +
            """{"type":"table"},{"type":"table",""" +
            """"content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell",""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"last"}]}]}]}]}]}"""
        const val FRAME_BESIDE_TABLE_DOCUMENT =
            """{"type":"doc","content":[{"type":"paragraph",""" +
                """"content":[{"type":"text","text":"before"}]},""" +
                """{"type":"table"},{"type":"table",""" +
                """"content":[{"type":"table_row",""" +
                """"content":[{"type":"table_cell",""" +
                """"content":[{"type":"paragraph",""" +
                """"content":[{"type":"text","text":"keep"}]}]}]}]},""" +
                """{"type":"paragraph","content":[{"type":"text",""" +
                """"text":"after"}]}]}"""
    }
}
