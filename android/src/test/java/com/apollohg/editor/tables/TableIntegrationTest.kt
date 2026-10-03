package com.apollohg.editor.tables

import android.app.Activity
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Rect
import android.graphics.RectF
import android.os.Looper
import android.util.Size
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.accessibility.AccessibilityNodeInfo
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.widget.FrameLayout
import androidx.core.view.accessibility.AccessibilityNodeInfoCompat
import com.apollohg.editor.EditorEditText
import com.apollohg.editor.EditorV2Adapter
import com.apollohg.editor.EditorV2CallResult
import com.apollohg.editor.EditorV2Registry
import com.apollohg.editor.NativeEditorExpoView
import com.apollohg.editor.NativeEditorExpoViewTestSupport
import com.apollohg.editor.RemoteSelectionOverlayView
import com.apollohg.editor.UniffiEditorV2Backend
import com.apollohg.editor.tableRecordsForTesting
import com.apollohg.editor.testExpoContext
import com.apollohg.editor.viewer.PreparedProseDrawingView
import com.apollohg.editor.viewer.RemoteTableCellSelection
import java.lang.ref.WeakReference
import java.time.Duration
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class TableIntegrationTest : NativeEditorExpoViewTestSupport() {
    @Test
    fun `large table frame coordinates match the engine for every cell`() {
        val created = UniffiEditorV2Backend.create(
            PlainTableFixture.CONFIG,
            null
        ) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                UniffiEditorV2Backend,
                JSONObject(created.value).getString("editorId"),
                false
            )
        )
        try {
            requireNotNull(
                adapter.setContentJson(
                    PlainTableFixture.document(
                        PlainTableFixture.LARGE_ROWS,
                        PlainTableFixture.LARGE_COLUMNS
                    )
                )
            )
            assertEquals(
                PlainTableFixture.LARGE_ROWS * PlainTableFixture.LARGE_COLUMNS,
                adapter.tableIndex.record(adapter.tableIndex.tableKeys.single())?.cells?.size
            )
            com.apollohg.editor.assertFramePositionsMatchEngine(adapter)
        } finally {
            adapter.destroy()
        }
    }

    private data class Peer(
        val clientId: String,
        val color: String,
        val anchor: Int,
        val head: Int,
        val cellRectangle: Pair<Int, Int>?,
        val resolvedAt: JSONObject? = null
    )

    private class Fixture(
        val view: NativeEditorExpoView,
        val adapter: EditorV2Adapter,
        val token: Long,
        val tableId: String,
        val viewport: Size
    ) {
        private val remote = RemoteTablePeer(adapter, REMOTE_REQUEST_ID_BASE)
        private var nextKeyEventTime = 0L

        val root: EditorEditText get() = view.richTextView.editorEditText
        val surface: EditorTableSurface get() = view.richTextView.editorTableSurface
        val drawing: PreparedProseDrawingView get() = surface.drawingView

        fun positions(): List<Int> = adapter.tableCellPositions(tableId)

        fun tablePos(): Int =
            requireNotNull(adapter.tableRecordsForTesting[tableId]).getInt("tablePos")

        fun presentedCell(position: Int): ViewerTablePresentedCell =
            drawing.presentedRealCell(tableId, position)

        fun visibleRect(cell: ViewerTablePresentedCell): RectF? =
            RectF(cell.bounds).takeIf { it.intersect(cell.clip) }

        fun remoteRects(selection: RemoteTableCellSelection): List<RectF> =
            requireNotNull(drawing.tableCellRects(selection.tableId, selection.sourceIndices))

        fun activeCellPosition(): Long? = view.richTextView.activeTableCellPosition

        fun relayout(viewport: Size = this.viewport) {
            view.measure(
                View.MeasureSpec.makeMeasureSpec(viewport.width, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(viewport.height, View.MeasureSpec.EXACTLY)
            )
            view.layout(0, 0, viewport.width, viewport.height)
            drawing.measure(
                View.MeasureSpec.makeMeasureSpec(root.width, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(root.height, View.MeasureSpec.EXACTLY)
            )
            drawing.layout(0, 0, drawing.measuredWidth, drawing.measuredHeight)
            shadowOf(Looper.getMainLooper()).idle()
        }

        fun setPeers(peers: List<Peer>) {
            val items = JSONArray()
            peers.forEach { peer ->
                val item = JSONObject().put("clientId", peer.clientId).put("anchor", peer.anchor)
                    .put("head", peer.head).put("color", peer.color).put("name", PEER_NAME)
                    .put("isFocused", true)
                peer.resolvedAt?.let { item.put("resolvedAt", it) }
                peer.cellRectangle?.let { (anchor, head) ->
                    item.put(
                        "cellRectangle",
                        JSONObject().put("anchorCell", anchor).put("headCell", head)
                    )
                }
                items.put(item)
            }
            view.setRemoteSelectionsJson(items.toString())
        }

        fun selectCells(anchor: Int, head: Int) {
            root.selectTableCells(adapter, anchor, head)
            assertTrue(
                "root did not adopt the cell selection",
                root.authoritativeCellSelectionActive
            )
            relayout()
        }

        fun applyRemoteCommand(command: JSONObject) = remote.applyCommand(command)

        fun remoteDocumentRevision(): ULong = remote.documentRevision()

        fun applyRemoteTextSelection(scalar: Int) = remote.applySelection(
            adapter.selectionEnvelope(
                scalar,
                scalar,
                REMOTE_SELECTION_AFFINITY
            ).getJSONObject("selection")
        )

        fun applyRemoteCellSelection(anchor: Int, head: Int) =
            remote.applySelection(documentCellSelection(anchor, head))

        fun deliverRemoteCommit() {
            view.applyRemoteCommitRefresh(token)
            relayout()
        }

        fun tapCell(position: Int) {
            val cell = presentedCell(position)
            val x = cell.bounds.centerX() + drawing.left
            val y = cell.bounds.centerY() + drawing.top
            listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP).forEachIndexed { index, action ->
                val event = MotionEvent.obtain(0, TOUCH_STEP_MS * index, action, x, y, 0)
                try {
                    view.richTextView.editorContentFrame.dispatchTouchEvent(event)
                } finally {
                    event.recycle()
                }
            }
            shadowOf(Looper.getMainLooper())
                .idleFor(Duration.ofMillis(ViewConfiguration.getDoubleTapTimeout().toLong()))
        }

        fun pressTab(shift: Boolean = false) {
            val input = view.richTextView.activeTextInput
            nextKeyEventTime += KEY_EVENT_STEP_MS
            assertTrue(
                input.dispatchKeyEvent(
                    KeyEvent(
                        nextKeyEventTime,
                        nextKeyEventTime,
                        KeyEvent.ACTION_DOWN,
                        KeyEvent.KEYCODE_TAB,
                        0,
                        if (shift) KeyEvent.META_SHIFT_ON else 0
                    )
                )
            )
        }

        fun menuItemIds(): List<Int> {
            val menu = requireNotNull(root.selectionActionMode) { "no action mode is showing" }.menu
            return (0 until menu.size()).map { menu.getItem(it).itemId }
        }

        fun clickMenuItem(id: Int) {
            val mode = requireNotNull(root.selectionActionMode)
            assertTrue("menu item $id was not handled", mode.menu.performIdentifierAction(id, 0))
        }

        fun render(): Bitmap {
            val bitmap = Bitmap.createBitmap(drawing.width, drawing.height, Bitmap.Config.ARGB_8888)
            drawing.draw(Canvas(bitmap))
            return bitmap
        }
    }

    private fun fallbackClients(fixture: Fixture): List<String> =
        fixture.view.richTextView.remoteSelectionDebugSnapshotsForTesting().map { it.clientId }

    @Test
    fun `remote rectangle resolves by index and fills cells instead of the cursor fallback`() =
        withTable(GRID_DOCUMENT) { fixture ->
            val positions = fixture.positions()
            val first = positions[GRID_FIRST]
            val second = positions[GRID_SECOND]

            fixture.setPeers(
                listOf(
                    Peer(
                        FIRST_PEER,
                        FIRST_PEER_COLOR,
                        first,
                        second,
                        first to second
                    )
                )
            )

            val remote = fixture.drawing.remoteTableCellSelections.single()
            assertEquals(fixture.tableId, remote.tableId)
            assertEquals(setOf(GRID_FIRST, GRID_SECOND), remote.sourceIndices)
            assertEquals(
                GRID_FIRST,
                fixture.adapter.tableIndex.cellIndexContainingDoc(remote.tableId, first.toUInt())
            )
            assertEquals(
                GRID_SECOND,
                fixture.adapter.tableIndex.cellIndexContainingDoc(remote.tableId, second.toUInt())
            )
            assertEquals(expectedPeerFill(FIRST_PEER_COLOR), remote.color)
            assertEquals(
                "the rectangle must use the presented cell frames",
                listOf(first, second).mapNotNull {
                    fixture.visibleRect(fixture.presentedCell(it))
                }.toSet(),
                fixture.remoteRects(remote).toSet()
            )
            assertEquals(
                "a drawn rectangle replaces the peer's cursor fallback",
                emptyList<String>(),
                fallbackClients(fixture)
            )

            fixture.setPeers(listOf(Peer(FIRST_PEER, FIRST_PEER_COLOR, first, second, null)))

            assertTrue(fixture.drawing.remoteTableCellSelections.isEmpty())
            assertEquals(
                "a peer without a rectangle keeps its ordinary cursor",
                listOf(FIRST_PEER),
                fallbackClients(fixture)
            )
        }

    @Test
    fun `unresolvable remote rectangle is dropped and only the cursor fallback remains`() =
        withTable(GRID_DOCUMENT) { fixture ->
            val first = fixture.positions()[GRID_FIRST]

            fixture.setPeers(
                listOf(
                    Peer(
                        FIRST_PEER,
                        FIRST_PEER_COLOR,
                        first,
                        first,
                        first + TEXT_OFFSET_INSIDE_CELL to first
                    )
                )
            )

            assertTrue(
                "an anchor that is not a real cell opening must not draw",
                fixture.drawing.remoteTableCellSelections.isEmpty()
            )
            assertEquals(listOf(FIRST_PEER), fallbackClients(fixture))
        }

    @Test
    fun `remote rectangle received before measurement appears after first layout`() =
        withTable(GRID_DOCUMENT, viewport = Size(0, VIEW_HEIGHT)) { fixture ->
            val first = fixture.positions()[GRID_FIRST]
            val frame = JSONObject().put("editorId", fixture.adapter.editorId.toString())
                .put("documentRevision", fixture.adapter.baseDocumentRevision.toString())
            fixture.setPeers(
                listOf(
                    Peer(
                        FIRST_PEER,
                        FIRST_PEER_COLOR,
                        first,
                        first,
                        first to first,
                        frame
                    )
                )
            )
            assertTrue(
                "unmeasured table must not display a rectangle",
                fixture.drawing.remoteTableCellSelections.isEmpty()
            )
            assertTrue(
                "pending rectangle must not fall back to a cursor",
                fallbackClients(fixture).isEmpty()
            )

            fixture.view.layoutParams = fixture.view.layoutParams.apply { width = VIEW_WIDTH }
            fixture.relayout(Size(VIEW_WIDTH, VIEW_HEIGHT))

            assertEquals(
                "first measured layout must retry pending presence without new props",
                setOf(GRID_FIRST),
                fixture.drawing.remoteTableCellSelections.single().sourceIndices
            )
            assertTrue(
                "the presented rectangle replaces the cursor",
                fallbackClients(fixture).isEmpty()
            )
        }

    @Test
    fun `remote rectangles wait for their resolved frame in either delivery order`() {
        val cell = """{"type":"table_cell","content":[{"type":"paragraph"}]}"""
        val row = """{"type":"table_row","content":[$cell]}"""
        val document = """{"type":"doc","content":[{"type":"table","content":[$row,$row]}]}"""
        for (nativeFirst in listOf(false, true)) {
            withTable(document) { fixture ->
                val positions = fixture.positions()
                val first = positions[0]
                val shifted = positions[1]
                val revision = fixture.adapter.baseDocumentRevision
                fun peer(opening: Int, revision: ULong) = Peer(
                    FIRST_PEER,
                    FIRST_PEER_COLOR,
                    opening,
                    opening,
                    opening to opening,
                    JSONObject().put(
                        "editorId",
                        fixture.adapter.editorId
                    ).put("documentRevision", revision.toString())
                )
                fixture.setPeers(listOf(peer(first, revision)))
                assertEquals(
                    setOf(0),
                    fixture.drawing.remoteTableCellSelections.single().sourceIndices
                )
                fixture.applyRemoteCellSelection(first, first)
                fixture.applyRemoteCommand(
                    JSONObject().put("type", "addTableRow").put("side", "before")
                )
                if (nativeFirst) {
                    fixture.deliverRemoteCommit()
                } else {
                    fixture.setPeers(listOf(peer(shifted, revision + 1u)))
                }
                assertTrue(
                    "nativeFirst=$nativeFirst: a mismatched frame must not highlight another cell",
                    fixture.drawing.remoteTableCellSelections.isEmpty()
                )
                assertTrue(
                    "mismatched frames must not fall back to stale cursors",
                    fallbackClients(fixture).isEmpty()
                )
                if (nativeFirst) {
                    fixture.setPeers(listOf(peer(shifted, revision + 1u)))
                } else {
                    fixture.deliverRemoteCommit()
                }
                assertEquals(
                    setOf(1),
                    fixture.drawing.remoteTableCellSelections.single().sourceIndices
                )
            }
        }
    }

    @Test
    fun `remote merge re-resolves the rectangle to the merged cell`() =
        withTable(GRID_DOCUMENT) { fixture ->
            val before = fixture.positions()
            val first = before[GRID_FIRST]
            val firstWidth = fixture.presentedCell(first).bounds.width()
            val secondWidth = fixture.presentedCell(before[GRID_SECOND]).bounds.width()
            fixture.setPeers(
                listOf(Peer(FIRST_PEER, FIRST_PEER_COLOR, first, first, first to first))
            )
            val unmerged = fixture.remoteRects(
                fixture.drawing.remoteTableCellSelections.single()
            ).single()
            assertEquals(firstWidth, unmerged.width(), GEOMETRY_TOLERANCE)

            fixture.applyRemoteCellSelection(first, before[GRID_SECOND])
            fixture.applyRemoteCommand(JSONObject().put("type", MERGE_TABLE_CELLS))
            fixture.deliverRemoteCommit()

            val cells = requireNotNull(
                fixture.adapter.tableRecordsForTesting[fixture.tableId]
            ).getJSONArray("cells")
            val mergedRecord = (0 until cells.length()).map(cells::getJSONObject).single {
                it.getInt("sourcePos") ==
                    first
            }
            assertEquals(
                "the remote merge must land",
                MERGED_COLSPAN,
                mergedRecord.getInt("colspan")
            )
            assertTrue(
                "revisionless coordinates expire with the frame",
                fixture.drawing.remoteTableCellSelections.isEmpty()
            )
            fixture.setPeers(
                listOf(
                    Peer(
                        FIRST_PEER,
                        FIRST_PEER_COLOR,
                        first,
                        first,
                        first to first,
                        JSONObject().put("editorId", fixture.adapter.editorId)
                            .put(
                                "documentRevision",
                                fixture.adapter.baseDocumentRevision.toString()
                            )
                    )
                )
            )
            val remote = fixture.drawing.remoteTableCellSelections.single()
            assertEquals(setOf(GRID_FIRST), remote.sourceIndices)
            val rect = fixture.remoteRects(remote).single()
            assertEquals(fixture.visibleRect(fixture.presentedCell(first)), rect)
            assertEquals(
                "the rectangle follows the merged cell",
                firstWidth + secondWidth,
                rect.width(),
                GEOMETRY_TOLERANCE
            )
        }

    @Test
    fun `expired peers take their rectangles with them`() = withTable(GRID_DOCUMENT) { fixture ->
        val positions = fixture.positions()
        val first = positions[GRID_FIRST]
        val last = positions[GRID_LAST]
        val firstPeer = Peer(FIRST_PEER, FIRST_PEER_COLOR, first, first, first to first)
        val secondPeer = Peer(SECOND_PEER, SECOND_PEER_COLOR, last, last, last to last)
        fixture.setPeers(listOf(firstPeer, secondPeer))
        assertEquals(
            listOf(setOf(positions.indexOf(first)), setOf(positions.indexOf(last))),
            fixture.drawing.remoteTableCellSelections.map { it.sourceIndices }
        )

        fixture.setPeers(listOf(secondPeer))

        assertEquals(
            listOf(setOf(positions.indexOf(last))),
            fixture.drawing.remoteTableCellSelections.map {
                it.sourceIndices
            }
        )
        assertEquals(
            expectedPeerFill(SECOND_PEER_COLOR),
            fixture.drawing.remoteTableCellSelections.single().color
        )

        fixture.setPeers(emptyList())

        assertTrue(fixture.drawing.remoteTableCellSelections.isEmpty())
        assertTrue(fallbackClients(fixture).isEmpty())
    }

    @Test
    @Config(qualifiers = WINDOW_COVERING_THE_EDITOR)
    fun `horizontal table scroll moves the rectangle with the cell and clips it to the table`() =
        withTable(WIDE_DOCUMENT) { fixture ->
            val second = fixture.positions()[GRID_SECOND]
            fixture.setPeers(
                listOf(
                    Peer(
                        FIRST_PEER,
                        FIRST_PEER_COLOR,
                        second,
                        second,
                        second to second
                    )
                )
            )
            val remote = fixture.drawing.remoteTableCellSelections.single()
            val before = fixture.presentedCell(second)
            assertTrue(
                "the second wide cell starts outside the table viewport: ${before.bounds} ${before.clip}",
                fixture.remoteRects(remote).isEmpty()
            )

            fixture.drawing.setTableLogicalOffset(before.surface.identity, HORIZONTAL_SCROLL)

            val after = fixture.presentedCell(second)
            assertEquals(
                before.bounds.left - HORIZONTAL_SCROLL,
                after.bounds.left,
                GEOMETRY_TOLERANCE
            )
            val rect = fixture.remoteRects(remote).single()
            assertEquals(
                "the rectangle moves with the scrolled cell",
                after.bounds.left,
                rect.left,
                GEOMETRY_TOLERANCE
            )
            assertEquals(
                "the rectangle is clipped to the table viewport",
                after.clip.right,
                rect.right,
                GEOMETRY_TOLERANCE
            )
            assertTrue(rect.right < after.bounds.right)
        }

    @Test
    @Config(qualifiers = RELEASE_VIEWPORT)
    fun `large table renders and presents only its viewport window`() {
        val maximumRetainedPresentations = PlainTableFixture.maximumPresentedCells(
            TableStyle(),
            RELEASE_VIEWPORT_WIDTH.toFloat(),
            RELEASE_VIEWPORT_HEIGHT.toFloat()
        )
        PlainTableFixture.TWENTY_THOUSAND_SLOT_SHAPES.forEach { (rows, columns) ->
            val label = "${rows}x$columns"
            lateinit var releasedView: WeakReference<NativeEditorExpoView>
            lateinit var releasedAdapter: WeakReference<EditorV2Adapter>
            withTable(
                PlainTableFixture.document(rows, columns),
                Size(RELEASE_VIEWPORT_WIDTH, RELEASE_VIEWPORT_HEIGHT)
            ) { fixture ->
                releasedView = WeakReference(fixture.view)
                releasedAdapter = WeakReference(fixture.adapter)
                val table = ViewerTablePresentation.surfaces(
                    requireNotNull(fixture.drawing.preparedLayout) {
                        "$label: the table is mounted"
                    }
                ).single()
                assertEquals("$label: every cell is prepared", rows * columns, table.cells.size)
                val drawn = mutableListOf<Int>()
                fixture.drawing.onMountedTableCellsDrawnForTesting = { drawn += it }
                val horizontalRoom = table.bounds.width() - table.hostViewportWidth
                listOf(0f, horizontalRoom / 2, horizontalRoom).forEach { offset ->
                    fixture.drawing.setTableLogicalOffset(table.identity, offset)
                    fixture.relayout()
                    fixture.drawing.draw(
                        Canvas(
                            Bitmap.createBitmap(
                                RELEASE_VIEWPORT_WIDTH,
                                RELEASE_VIEWPORT_HEIGHT,
                                Bitmap.Config.ARGB_8888
                            )
                        )
                    )
                    val presented = fixture.drawing.presentedTableCells()
                    val visible = Rect().also { fixture.drawing.getLocalVisibleRect(it) }
                    println(
                        "$label at table offset $offset: ${presented.size} presented, " +
                            "${drawn.lastOrNull()} drawn, visible $visible, bound $maximumRetainedPresentations"
                    )
                    assertTrue(
                        "$label: the presentation is bounded by the viewport window, not the table",
                        presented.size <= maximumRetainedPresentations
                    )
                    assertTrue(
                        "$label: drawing mounts only the window",
                        requireNotNull(drawn.lastOrNull()) <= maximumRetainedPresentations
                    )
                    assertTrue(
                        "$label: the cell under the viewport centre is presented",
                        presented.any {
                            it.bounds.contains(visible.exactCenterX(), visible.exactCenterY())
                        }
                    )
                }
            }
            shadowOf(Looper.getMainLooper()).idleFor(TEARDOWN_SETTLE)
            repeat(GC_ATTEMPTS) {
                System.gc()
                System.runFinalization()
            }
            assertNull("$label: the torn-down editor view is collectable", releasedView.get())
            assertNull("$label: the destroyed adapter is collectable", releasedAdapter.get())
        }
    }

    @Test
    @Config(qualifiers = RELEASE_VIEWPORT)
    fun `large table screen reader focus walks past the window with headers and stable ids`() =
        withTable(
            PlainTableFixture.document(
                PlainTableFixture.LARGE_ROWS,
                PlainTableFixture.LARGE_COLUMNS
            ),
            Size(RELEASE_VIEWPORT_WIDTH, RELEASE_VIEWPORT_HEIGHT)
        ) { fixture ->
            val drawing = fixture.drawing
            val screen = Rect(0, 0, RELEASE_VIEWPORT_WIDTH, RELEASE_VIEWPORT_HEIGHT)
            drawing.accessibilityVisibilityForTesting =
                { bounds -> Rect.intersects(bounds, screen) }
            val provider = drawing.accessibilityNodeProvider
            val surface = ViewerTablePresentation.surfaces(
                requireNotNull(drawing.preparedLayout)
            ).single()
            fun location(row: Int, column: Int) = requireNotNull(
                drawing.tableAccessibilityLocation(
                    surface,
                    row * PlainTableFixture.LARGE_COLUMNS + column
                )
            ) { "cell $row,$column has an accessibility node" }
            val firstBodyId = location(1, 0).cellNodeId
            PlainTableFixture.ACCESSIBILITY_WALK_ROWS.forEach { row ->
                val column = row % PlainTableFixture.LARGE_COLUMNS
                val id = location(row, column).cellNodeId
                assertTrue(
                    "focus reaches row $row",
                    provider.performAction(
                        id,
                        AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS,
                        null
                    )
                )
                val info = requireNotNull(provider.createAccessibilityNodeInfo(id))
                val item = requireNotNull(AccessibilityNodeInfoCompat.wrap(info).collectionItemInfo)
                println(
                    "row $row column $column: id $id focused ${info.isAccessibilityFocused} " +
                        "rowIndex ${item.rowIndex} title '${item.columnTitle}' text '${info.text}'"
                )
                assertTrue("row $row keeps focus after its reveal", info.isAccessibilityFocused)
                assertEquals(row, item.rowIndex)
                assertEquals(
                    "row $row announces its column header",
                    PlainTableFixture.CELL_TEXT,
                    item.columnTitle
                )
                assertEquals(
                    "row $row is read from its own text",
                    PlainTableFixture.CELL_TEXT,
                    info.text?.toString()
                )
            }
            assertEquals(
                "cell node ids do not shift while scrolling",
                firstBodyId,
                location(1, 0).cellNodeId
            )
        }

    @Test
    fun `right to left table mirrors the remote rectangle`() = withTable(GRID_DOCUMENT) { fixture ->
        fixture.view.richTextView.tableDirection = TableLayoutDirection.RIGHT_TO_LEFT
        fixture.relayout()
        val positions = fixture.positions()
        val first = positions[GRID_FIRST]
        fixture.setPeers(
            listOf(Peer(FIRST_PEER, FIRST_PEER_COLOR, first, first, first to first))
        )

        val firstCell = fixture.presentedCell(first)
        val secondCell = fixture.presentedCell(positions[GRID_SECOND])
        assertTrue(firstCell.surface.isRightToLeft)
        assertTrue(
            "the first column sits on the right",
            firstCell.bounds.left > secondCell.bounds.left
        )
        assertEquals(
            fixture.visibleRect(firstCell),
            fixture.remoteRects(fixture.drawing.remoteTableCellSelections.single()).single()
        )
    }

    @Test
    fun `owner rebinding re-resolves the rectangle against the new owner`() =
        withTable(GRID_DOCUMENT) { fixture ->
            val first = fixture.positions()[GRID_FIRST]
            fixture.setPeers(
                listOf(Peer(FIRST_PEER, FIRST_PEER_COLOR, first, first, first to first))
            )
            assertEquals(1, fixture.drawing.remoteTableCellSelections.size)

            fixture.view.setEditorId(0L)
            assertTrue(
                "an unbound view draws no presence",
                fixture.drawing.remoteTableCellSelections.isEmpty()
            )

            fixture.view.setEditorId(fixture.token)
            fixture.relayout()
            val restored = fixture.drawing.remoteTableCellSelections.single()
            assertEquals(setOf(GRID_FIRST), restored.sourceIndices)
            assertEquals(1, fixture.remoteRects(restored).size)

            val created = UniffiEditorV2Backend.create(
                PlainTableFixture.CONFIG,
                null
            ) as EditorV2CallResult.Ok
            val other = requireNotNull(
                EditorV2Adapter.attach(
                    UniffiEditorV2Backend,
                    JSONObject(created.value).getString("editorId"),
                    false
                )
            )
            val otherToken = EditorV2Registry.register(other)
            try {
                val update = requireNotNull(other.setContentJson(GRID_DOCUMENT))
                fixture.view.setEditorId(otherToken)
                assertTrue(fixture.root.applyUpdateJSON(update))
                fixture.relayout()
                val otherCells = other.tableRecordsForTesting.values.single().getJSONArray("cells")
                val otherOpenings = (0 until otherCells.length()).map {
                    otherCells.getJSONObject(it).getInt("sourcePos")
                }
                assertTrue(
                    "the new editor deliberately reuses the same opening",
                    first in otherOpenings
                )
                assertTrue(
                    "an equal opening in another editor must not inherit presence",
                    fixture.drawing.remoteTableCellSelections.isEmpty()
                )
                assertTrue(fallbackClients(fixture).isEmpty())
            } finally {
                fixture.view.setEditorId(fixture.token)
                EditorV2Registry.remove(other.editorId)
                other.destroy()
            }
        }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = WINDOW_COVERING_THE_EDITOR)
    fun `remote rectangle is painted behind local handles and the active cell input`() =
        withTable(GRID_DOCUMENT) { fixture ->
            val positions = fixture.positions()
            val first = positions[GRID_FIRST]
            val second = positions[GRID_SECOND]
            val leftColumnBottom = positions[GRID_LEFT_COLUMN_BOTTOM]
            fixture.selectCells(first, leftColumnBottom)
            val handles = fixture.drawing.selectionHandles()
            assertEquals(
                "both handles of the left-column selection are visible: $handles",
                2,
                handles.size
            )
            val cell = fixture.presentedCell(first)
            val interiorX = cell.bounds.centerX().toInt()
            val interiorY = (cell.bounds.bottom - PIXEL_INSET_FROM_CELL_BOTTOM).toInt()
            val localOnly = fixture.render()

            fixture.setPeers(
                listOf(
                    Peer(
                        FIRST_PEER,
                        FIRST_PEER_COLOR,
                        first,
                        leftColumnBottom,
                        first to leftColumnBottom
                    )
                )
            )
            assertEquals(1, fixture.drawing.remoteTableCellSelections.size)
            val withRemote = fixture.render()

            try {
                assertNotEquals(
                    "the remote fill must reach the cell",
                    localOnly.getPixel(interiorX, interiorY),
                    withRemote.getPixel(interiorX, interiorY)
                )
                handles.forEach { handle ->
                    assertEquals(
                        "the ${handle.role} handle must cover the remote fill",
                        localOnly.getPixel(handle.x.toInt(), handle.y.toInt()),
                        withRemote.getPixel(handle.x.toInt(), handle.y.toInt())
                    )
                }
            } finally {
                localOnly.recycle()
                withRemote.recycle()
            }

            fixture.selectCells(first, second)
            fixture.tapCell(positions[GRID_LAST])
            val input = fixture.view.richTextView.activeTextInput
            assertTrue("the tap edits a cell", input !== fixture.root)
            val frame = fixture.view.richTextView.editorContentFrame
            assertSame(frame, input.parent)
            assertSame(frame, fixture.drawing.parent)
            assertTrue(
                "the active cell input sits above the painted rectangle",
                frame.indexOfChild(input) > frame.indexOfChild(fixture.drawing)
            )
        }

    @Test
    fun `keyboard composition clipboard and menu each mutate once and exclude synthetic slots`() =
        withTable(IRREGULAR_DOCUMENT) { fixture ->
            clipboard().clearPrimaryClip()
            var positions = fixture.positions()
            val startRevision = fixture.adapter.baseDocumentRevision

            fixture.tapCell(positions[TALL_CELL])
            assertEquals(positions[TALL_CELL].toLong(), fixture.activeCellPosition())
            listOf(WIDE_CELL, LATER_CELL).forEach { expected ->
                fixture.pressTab()
                assertEquals(
                    "Tab must reach cell $expected",
                    positions[expected].toLong(),
                    fixture.activeCellPosition()
                )
            }
            fixture.pressTab(shift = true)
            assertEquals(positions[WIDE_CELL].toLong(), fixture.activeCellPosition())
            assertEquals(
                "focus moves never mutate",
                startRevision,
                fixture.adapter.baseDocumentRevision
            )

            val input = fixture.view.richTextView.activeTextInput
            input.setSelection(0)
            val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(connection.setComposingText(COMPOSITION_TEXT, 1))
            assertEquals(
                "marked text is not a mutation",
                startRevision,
                fixture.adapter.baseDocumentRevision
            )
            assertTrue(connection.finishComposingText())
            val composedRevision = fixture.adapter.baseDocumentRevision
            assertEquals(
                "a committed composition is exactly one mutation",
                startRevision + 1u,
                composedRevision
            )
            val composed = requireNotNull(fixture.adapter.documentJson())
            assertTrue(composed, composed.contains(textNode(COMPOSITION_TEXT + WIDE_TEXT)))

            positions = fixture.positions()
            val wide = positions[WIDE_CELL]
            val later = positions[LATER_CELL]
            fixture.root.requestFocus()
            fixture.selectCells(wide, later)
            val wideCell = fixture.presentedCell(wide)
            val laterCell = fixture.presentedCell(later)
            val gap = RectF(
                laterCell.bounds.right,
                laterCell.bounds.top,
                wideCell.bounds.right,
                laterCell.bounds.bottom
            )
                .apply { inset(1f, 1f) }
            assertFalse("the gap sits after the last real cell of the second row", gap.isEmpty)
            val rectangle = setOf(WIDE_CELL, LATER_CELL)
            assertEquals(rectangle, fixture.drawing.selectedTableCellSourceIndices[fixture.tableId])
            val selectedRects =
                requireNotNull(fixture.drawing.selectedTableCellRects(fixture.tableId))
            assertEquals(rectangle.size, selectedRects.size)
            assertFalse(
                "the gap slot is never selected",
                selectedRects.any {
                    RectF.intersects(it, gap)
                }
            )

            fixture.setPeers(listOf(Peer(FIRST_PEER, FIRST_PEER_COLOR, wide, later, wide to later)))
            val remote = fixture.drawing.remoteTableCellSelections.single()
            assertEquals(
                "presence shares the local effective rectangle",
                rectangle,
                remote.sourceIndices
            )
            assertFalse(fixture.remoteRects(remote).any { RectF.intersects(it, gap) })

            assertTrue(
                fixture.root.dispatchKeyEvent(
                    KeyEvent(
                        0L,
                        0L,
                        KeyEvent.ACTION_DOWN,
                        KeyEvent.KEYCODE_C,
                        0,
                        KeyEvent.META_CTRL_ON
                    )
                )
            )
            assertEquals(
                IRREGULAR_RECTANGLE_TSV,
                requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString()
            )
            assertEquals(
                "copy never mutates",
                composedRevision,
                fixture.adapter.baseDocumentRevision
            )

            fixture.tapCell(later)
            assertTrue(
                "a tap inside the selection opens the cell menu",
                fixture.surface.isCellEditMenuVisible
            )
            assertEquals(CELL_MENU_ITEMS, fixture.menuItemIds())
            val beforeCut = requireNotNull(fixture.adapter.documentJson())

            fixture.clickMenuItem(android.R.id.cut)

            assertEquals(
                "cut is exactly one mutation",
                composedRevision + 1u,
                fixture.adapter.baseDocumentRevision
            )
            val cut = requireNotNull(fixture.adapter.documentJson())
            assertFalse(
                cut,
                cut.contains(textNode(COMPOSITION_TEXT + WIDE_TEXT)) ||
                    cut.contains(textNode(LATER_TEXT))
            )
            assertTrue(cut, cut.contains(textNode(TALL_TEXT)))
            assertEquals(
                IRREGULAR_RECTANGLE_TSV,
                requireNotNull(clipboard().primaryClip).getItemAt(0).text.toString()
            )
            assertTrue(
                "cut keeps the cell selection",
                fixture.root.authoritativeCellSelectionActive
            )

            fixture.relayout()
            fixture.tapCell(fixture.positions()[LATER_CELL])
            assertTrue(fixture.surface.isCellEditMenuVisible)
            fixture.clickMenuItem(android.R.id.paste)

            assertEquals(
                "a paste the planner refuses over the gap is no mutation at all",
                composedRevision + 1u,
                fixture.adapter.baseDocumentRevision
            )
            assertEquals(cut, fixture.adapter.documentJson())
            assertTrue(fixture.root.applyUpdateJSON(requireNotNull(fixture.adapter.undo())))
            assertTrue(
                "one undo restores the whole cut",
                sameJson(
                    JSONObject(beforeCut),
                    JSONObject(requireNotNull(fixture.adapter.documentJson()))
                )
            )
            assertTrue(
                "the composition entry remains",
                requireNotNull(fixture.adapter.historyCanUndo())
            )
            fixture.relayout()
            assertEquals(rectangle, fixture.drawing.selectedTableCellSourceIndices[fixture.tableId])
        }

    @Test
    fun `remote deletion under cell menu clears selection and keeps cursor`() =
        withTable(IRREGULAR_DOCUMENT) { fixture ->
            val positions = fixture.positions()
            fixture.selectCells(positions[TALL_CELL], positions[WIDE_CELL])
            fixture.surface.presentCellEditMenu()
            assertTrue(fixture.surface.isCellEditMenuVisible)
            val staleRectangle = positions[WIDE_CELL] to positions[LATER_CELL]
            fixture.setPeers(
                listOf(
                    Peer(
                        FIRST_PEER,
                        FIRST_PEER_COLOR,
                        staleRectangle.first,
                        staleRectangle.second,
                        staleRectangle
                    )
                )
            )
            assertEquals(1, fixture.drawing.remoteTableCellSelections.size)
            val tablePos = fixture.tablePos()
            val revision = fixture.adapter.baseDocumentRevision

            fixture.applyRemoteCommand(
                JSONObject().put("type", DELETE_TABLE).put("tablePos", tablePos)
            )
            fixture.deliverRemoteCommit()
            fixture.setPeers(
                listOf(Peer(FIRST_PEER, FIRST_PEER_COLOR, tablePos, tablePos, staleRectangle))
            )

            assertTrue(
                "the remote peer deleted the table",
                fixture.adapter.tableRecordsForTesting.isEmpty()
            )
            assertEquals(
                "only the remote change was applied",
                revision + 1u,
                fixture.adapter.baseDocumentRevision
            )
            assertFalse(fixture.root.authoritativeCellSelectionActive)
            assertTrue(fixture.drawing.selectedTableCellSourceIndices.isEmpty())
            assertFalse("the menu closes with its selection", fixture.surface.isCellEditMenuVisible)
            assertTrue(
                "the dead rectangle is removed",
                fixture.drawing.remoteTableCellSelections.isEmpty()
            )
            assertEquals(
                "the peer keeps its ordinary cursor",
                listOf(FIRST_PEER),
                fallbackClients(fixture)
            )
        }

    @Test
    fun `remote table deletion during cell composition cancels it without a mutation`() =
        withTable(IRREGULAR_DOCUMENT) { fixture ->
            val composing = composeStaleText(fixture, fixture.positions()[TALL_CELL])
            val revision = fixture.adapter.baseDocumentRevision

            fixture.applyRemoteCommand(
                JSONObject().put("type", DELETE_TABLE).put("tablePos", fixture.tablePos())
            )
            val remoteDocument = requireNotNull(fixture.adapter.documentJson())
            fixture.deliverRemoteCommit()
            composing.connection.finishComposingText()
            fixture.relayout()

            assertCancelledComposition(fixture, composing, remoteDocument, revision)
            assertTrue(fixture.adapter.tableRecordsForTesting.isEmpty())
        }

    @Test
    fun `remote row deletion during cell composition cancels it without a mutation`() =
        withTable(GRID_DOCUMENT) { fixture ->
            val lastRowCell = fixture.positions()[GRID_LAST]
            val composing = composeStaleText(fixture, lastRowCell)
            val revision = fixture.adapter.baseDocumentRevision

            fixture.applyRemoteCellSelection(lastRowCell, lastRowCell)
            fixture.applyRemoteCommand(JSONObject().put("type", DELETE_TABLE_ROWS))
            val remoteDocument = requireNotNull(fixture.adapter.documentJson())
            assertFalse(
                "the remote peer removed the composing cell's row: $remoteDocument",
                remoteDocument.contains(textNode(GRID_LAST_TEXT))
            )
            fixture.deliverRemoteCommit()
            composing.connection.finishComposingText()
            fixture.relayout()

            assertCancelledComposition(fixture, composing, remoteDocument, revision)
            assertEquals("the first row survives", GRID_SECOND + 1, fixture.positions().size)
        }

    @Test
    fun `remote edit elsewhere during cell composition keeps composing in the same cell`() =
        withTable(IRREGULAR_DOCUMENT) { fixture ->
            val tall = fixture.positions()[TALL_CELL]
            val composing = composeStaleText(fixture, tall)
            val revision = fixture.adapter.baseDocumentRevision

            fixture.applyRemoteTextSelection(REMOTE_PROSE_SCALAR)
            fixture.applyRemoteCommand(
                JSONObject().put("type", INSERT_TEXT).put("text", REMOTE_PROSE_TEXT)
            )
            fixture.deliverRemoteCommit()

            assertTrue(
                "a remote edit outside the cell must not end the composition",
                composing.input.hasPendingCompositionForExternalRefresh()
            )
            assertSame(composing.input, fixture.view.richTextView.activeTextInput)
            assertEquals(tall.toLong(), fixture.activeCellPosition())
            composing.connection.finishComposingText()
            fixture.relayout()

            val document = requireNotNull(fixture.adapter.documentJson())
            assertTrue(
                document,
                document.contains(textNode(REMOTE_PROSE_TEXT + PROSE_BEFORE_TABLE_TEXT))
            )
            assertTrue(
                "the composition lands in its moved cell: $document",
                document.contains(textNode(STALE_COMPOSITION_TEXT + TALL_TEXT))
            )
            assertEquals(
                "one remote edit and one composition commit",
                revision + 2u,
                fixture.adapter.baseDocumentRevision
            )
            assertFalse(composing.input.hasPendingCompositionForExternalRefresh())
        }

    @Test
    fun `remote header toggle of the composing cell keeps composing in that cell`() =
        withTable(IRREGULAR_DOCUMENT) { fixture ->
            val tall = fixture.positions()[TALL_CELL]
            val composing = composeStaleText(fixture, tall)
            val revision = fixture.adapter.baseDocumentRevision

            fixture.applyRemoteCellSelection(tall, tall)
            fixture.applyRemoteCommand(
                JSONObject().put("type", TOGGLE_TABLE_HEADER).put("target", HEADER_TARGET_CELL)
            )
            val remoteDocument = requireNotNull(fixture.adapter.documentJson())
            assertTrue(
                "the remote peer retyped the cell: $remoteDocument",
                remoteDocument.contains(HEADER_CELL_NODE)
            )
            fixture.deliverRemoteCommit()
            assertTrue(composing.input.hasPendingCompositionForExternalRefresh())
            composing.connection.finishComposingText()
            fixture.relayout()

            val document = requireNotNull(fixture.adapter.documentJson())
            assertTrue(
                "the composition lands in the retyped cell: $document",
                document.contains(textNode(STALE_COMPOSITION_TEXT + TALL_TEXT))
            )
            assertTrue(document, document.contains(HEADER_CELL_NODE))
            assertEquals(
                "one remote retype and one composition commit",
                revision + 2u,
                fixture.adapter.baseDocumentRevision
            )
            assertFalse(composing.input.hasPendingCompositionForExternalRefresh())
        }

    @Test
    fun `remote text edit and header replacement release composition and allow fresh typing`() =
        withTable(IRREGULAR_DOCUMENT) { fixture ->
            val tall = fixture.positions()[TALL_CELL]
            val composing = composeStaleText(fixture, tall)
            val scalar =
                requireNotNull(composing.input.tableCellPositionMap?.globalScalarForLocalScalar(0))
            fixture.applyRemoteTextSelection(scalar)
            fixture.applyRemoteCommand(
                JSONObject().put("type", INSERT_TEXT).put("text", REMOTE_PROSE_TEXT)
            )
            fixture.applyRemoteCommand(
                JSONObject().put("type", TOGGLE_TABLE_HEADER).put("target", HEADER_TARGET_CELL)
            )
            val remoteDocument = requireNotNull(fixture.adapter.documentJson())
            val remoteRevision = fixture.remoteDocumentRevision()
            assertTrue(
                remoteDocument,
                remoteDocument.contains(textNode(REMOTE_PROSE_TEXT + TALL_TEXT))
            )
            assertTrue(remoteDocument, remoteDocument.contains(HEADER_CELL_NODE))
            fixture.deliverRemoteCommit()
            composing.connection.finishComposingText()
            fixture.relayout()
            assertEquals(remoteDocument, fixture.adapter.documentJson())
            assertEquals(remoteRevision, fixture.adapter.baseDocumentRevision)
            assertFalse(composing.input.hasPendingCompositionForExternalRefresh())
            assertTrue(
                "the stale IME session retired",
                composing.input.activeInputConnection !== composing.connection
            )
            composing.connection.setComposingText(STALE_COMPOSITION_TEXT, 1)
            assertFalse(composing.input.text.toString().contains(STALE_COMPOSITION_TEXT))
            assertEquals(
                "the retired connection cannot mutate",
                remoteDocument,
                fixture.adapter.documentJson()
            )
            fixture.tapCell(fixture.positions()[TALL_CELL])
            val fresh = fixture.view.richTextView.activeTextInput
            assertTrue("a fresh cell owns input", fresh !== fixture.root)
            fresh.setSelection(0)
            val connection = requireNotNull(fresh.onCreateInputConnection(EditorInfo()))
            assertTrue(connection.commitText(COMPOSITION_TEXT, 1))
            fixture.relayout()
            val document = requireNotNull(fixture.adapter.documentJson())
            assertTrue(
                document,
                document.contains(textNode(COMPOSITION_TEXT + REMOTE_PROSE_TEXT + TALL_TEXT))
            )
            assertEquals(remoteRevision + 1u, fixture.adapter.baseDocumentRevision)
        }

    private class Composition(val input: EditorEditText, val connection: InputConnection)

    private fun composeStaleText(fixture: Fixture, cellPosition: Int): Composition {
        fixture.tapCell(cellPosition)
        val input = fixture.view.richTextView.activeTextInput
        assertTrue("a cell input must own the composition", input !== fixture.root)
        input.setSelection(0)
        val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
        assertTrue(connection.setComposingText(STALE_COMPOSITION_TEXT, 1))
        assertTrue(input.hasPendingCompositionForExternalRefresh())
        return Composition(input, connection)
    }

    private fun assertCancelledComposition(
        fixture: Fixture,
        composing: Composition,
        remoteDocument: String,
        revision: ULong
    ) {
        assertEquals(
            "the stale composition must not land anywhere",
            remoteDocument,
            fixture.adapter.documentJson()
        )
        assertEquals(
            "only the remote change was applied",
            revision + 1u,
            fixture.adapter.baseDocumentRevision
        )
        assertFalse(composing.input.hasPendingCompositionForExternalRefresh())
        assertNull(
            "the editor retired the composing IME connection",
            composing.input.activeInputConnection
        )
        composing.connection.setComposingText(STALE_COMPOSITION_TEXT, 1)
        val editable = requireNotNull(composing.input.text)
        assertEquals(
            "the released input retired its IME session, so a late composing update is ignored",
            -1,
            BaseInputConnection.getComposingSpanStart(editable)
        )
        assertFalse(
            "the marked text is removed from the released input: $editable",
            editable.toString().contains(STALE_COMPOSITION_TEXT)
        )
        assertEquals(
            "a late composing update is not a mutation",
            remoteDocument,
            fixture.adapter.documentJson()
        )
        assertSame(
            "the dead cell input is released",
            fixture.root,
            fixture.view.richTextView.activeTextInput
        )
        assertFalse(
            fixture.root.text.toString(),
            fixture.root.text.toString().contains(STALE_COMPOSITION_TEXT)
        )
    }

    private fun withTable(
        document: String,
        viewport: Size = Size(VIEW_WIDTH, VIEW_HEIGHT),
        block: (Fixture) -> Unit
    ) {
        val activity = Robolectric.buildActivity(Activity::class.java).setup()
        val created = UniffiEditorV2Backend.create(
            PlainTableFixture.CONFIG,
            null
        ) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                UniffiEditorV2Backend,
                JSONObject(created.value).getString("editorId"),
                false
            )
        )
        val token = EditorV2Registry.register(adapter)
        try {
            val expo = testExpoContext(activity.get())
            val view = NativeEditorExpoView(expo.context, expo.appContext)
            view.onFocusChangeForTesting = {}
            view.onAddonEventForTesting = {}
            view.onEditorReadyForTesting = {}
            view.onEditorUpdateForTesting = {}
            view.onSelectionChangeForTesting = {}
            view.onContentHeightChangeForTesting = {}
            view.onAtomLayoutForTesting = {}
            view.onTableSelectionGeometryForTesting = {}
            activity.get().setContentView(
                FrameLayout(activity.get()).apply {
                    addView(view, FrameLayout.LayoutParams(viewport.width, viewport.height))
                }
            )
            view.setAttachedToNativeWindowForTesting(true)
            view.setEditorId(token)
            assertTrue(
                view.richTextView.editorEditText.applyUpdateJSON(
                    requireNotNull(adapter.setContentJson(document))
                )
            )
            val tableId = requireNotNull(
                adapter.tableRecordsForTesting.entries.firstOrNull {
                    !it.value.optBoolean("readOnlyDescendants", true)
                }?.key
            )
            val fixture = Fixture(view, adapter, token, tableId, viewport)
            fixture.relayout()
            fixture.root.requestFocus()
            try {
                block(fixture)
            } finally {
                activity.pause().stop().destroy()
            }
        } finally {
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    private fun expectedPeerFill(color: String): Int {
        val opaque = Color.parseColor(color)
        return Color.argb(
            (COLOR_CHANNEL_MAX * RemoteSelectionOverlayView.SELECTION_ALPHA).toInt(),
            Color.red(opaque),
            Color.green(opaque),
            Color.blue(opaque)
        )
    }

    private fun clipboard(): ClipboardManager = RuntimeEnvironment.getApplication()
        .getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager

    private fun textNode(text: String): String = "\"text\":\"$text\""

    private fun sameJson(left: Any?, right: Any?): Boolean = when {
        left is JSONObject && right is JSONObject ->
            left.keys().asSequence().toSet() == right.keys().asSequence().toSet() &&
                left.keys().asSequence().all { sameJson(left.get(it), right.get(it)) }

        left is JSONArray && right is JSONArray ->
            left.length() == right.length() &&
                (0 until left.length()).all { sameJson(left.get(it), right.get(it)) }

        left is Number && right is Number -> left.toDouble() == right.toDouble()

        else -> left == right
    }

    private companion object {
        const val VIEW_WIDTH = 900
        const val VIEW_HEIGHT = 500
        const val RELEASE_VIEWPORT_WIDTH = 390
        const val RELEASE_VIEWPORT_HEIGHT = 844
        const val RELEASE_VIEWPORT =
            "w${RELEASE_VIEWPORT_WIDTH}dp-h${RELEASE_VIEWPORT_HEIGHT}dp-mdpi"
        const val GC_ATTEMPTS = 3
        val TEARDOWN_SETTLE: Duration = Duration.ofSeconds(1)
        const val WINDOW_COVERING_THE_EDITOR = "w1000dp-h700dp"
        const val TOUCH_STEP_MS = 20L
        const val KEY_EVENT_STEP_MS = 100L
        const val REMOTE_REQUEST_ID_BASE = 22_000_000L
        const val FIRST_PEER = "7"
        const val SECOND_PEER = "9"
        const val FIRST_PEER_COLOR = "#FF0000"
        const val SECOND_PEER_COLOR = "#0000FF"
        const val PEER_NAME = "Remote"
        const val COLOR_CHANNEL_MAX = 255f
        const val HORIZONTAL_SCROLL = 300f
        const val GEOMETRY_TOLERANCE = 0.5f
        const val PIXEL_INSET_FROM_CELL_BOTTOM = 3f
        const val COMPOSITION_TEXT = "Z"
        const val STALE_COMPOSITION_TEXT = "Q"
        const val REMOTE_PROSE_TEXT = "R"
        const val REMOTE_PROSE_SCALAR = 0
        const val REMOTE_SELECTION_AFFINITY = "before"
        const val IRREGULAR_RECTANGLE_TSV = "Zwide\t\nlater\t"
        const val GRID_FIRST = 0
        const val GRID_SECOND = 1
        const val GRID_LEFT_COLUMN_BOTTOM = 2
        const val GRID_LAST = 3
        const val TALL_CELL = 0
        const val WIDE_CELL = 1
        const val LATER_CELL = 2
        const val MERGED_COLSPAN = 2
        const val TEXT_OFFSET_INSIDE_CELL = 2
        const val DELETE_TABLE = "deleteTable"
        const val DELETE_TABLE_ROWS = "deleteTableRows"
        const val INSERT_TEXT = "insertText"
        const val TOGGLE_TABLE_HEADER = "toggleTableHeader"
        const val HEADER_TARGET_CELL = "cell"
        const val HEADER_CELL_NODE = "\"type\":\"table_header\""
        const val MERGE_TABLE_CELLS = "mergeTableCells"
        val CELL_MENU_ITEMS = listOf(android.R.id.cut, android.R.id.copy, android.R.id.paste)

        const val GRID_LAST_TEXT = "D"
        const val PROSE_BEFORE_TABLE_TEXT = "before"
        const val TALL_TEXT = "tall"
        const val WIDE_TEXT = "wide"
        const val LATER_TEXT = "later"
        const val GRID_DOCUMENT = """{"type":"doc","content":[{"type":"table",""" +
            """"content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell",""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"A"}]}]},""" +
            """{"type":"table_cell","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"B"}]}]}]},""" +
            """{"type":"table_row","content":[{"type":"table_cell",""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"C"}]}]},""" +
            """{"type":"table_cell","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"D"}]}]}]}]},""" +
            """{"type":"paragraph","content":[{"type":"text",""" +
            """"text":"after"}]}]}"""
        const val SHIFTED_GRID_DOCUMENT = """{"type":"doc","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"shifted"}]},""" +
            """{"type":"table","content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell",""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"A"}]}]},""" +
            """{"type":"table_cell","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"B"}]}]}]},""" +
            """{"type":"table_row","content":[{"type":"table_cell",""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"C"}]}]},""" +
            """{"type":"table_cell","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"D"}]}]}]}]},""" +
            """{"type":"paragraph","content":[{"type":"text",""" +
            """"text":"after"}]}]}"""
        const val IRREGULAR_DOCUMENT = """{"type":"doc","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"before"}]},""" +
            """{"type":"table","content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell","attrs":{"rowspan":2},""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"tall"}]}]},""" +
            """{"type":"table_cell","attrs":{"colspan":2},""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"wide"}]}]}]},""" +
            """{"type":"table_row","content":[{"type":"table_cell",""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"later"}]}]}]}]},""" +
            """{"type":"paragraph","content":[{"type":"text",""" +
            """"text":"after"}]}]}"""
        const val WIDE_DOCUMENT = """{"type":"doc","content":[{"type":"table",""" +
            """"content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell",""" +
            """"attrs":{"colwidth":[1000]},""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"one"}]}]},""" +
            """{"type":"table_cell","attrs":{"colwidth":[1000]},""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"two"}]}]}]}]},""" +
            """{"type":"paragraph","content":[{"type":"text",""" +
            """"text":"after"}]}]}"""
    }
}
