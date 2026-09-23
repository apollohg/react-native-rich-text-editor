package com.apollohg.editor

import android.app.Activity
import android.app.Instrumentation
import android.content.Context
import android.graphics.Bitmap
import android.graphics.Color
import android.os.SystemClock
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.widget.FrameLayout
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.LargeTest
import androidx.test.filters.SdkSuppress
import androidx.test.platform.app.InstrumentationRegistry
import com.apollohg.editor.viewer.PreparedProseDrawingView
import expo.modules.core.ModuleRegistry
import expo.modules.kotlin.AppContext
import expo.modules.kotlin.ModulesProvider
import expo.modules.kotlin.modules.Module
import java.lang.ref.WeakReference
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
@LargeTest
class NativeDeviceTableCellTest {
    private val instrumentation: Instrumentation = InstrumentationRegistry.getInstrumentation()

    @Test
    @SdkSuppress(minSdkVersion = 29)
    fun authoritativeCellRectangleRetiresInputAndReturnsToTextOnTap() = withEditor { fixture ->
        fixture.tapCell(0)
        fixture.onActivity {
            val stale = requireNotNull(fixture.cellInput().onCreateInputConnection(EditorInfo()))
            val before = fixture.adapter.documentJson()
            val revision = fixture.adapter.baseDocumentRevision
            val cells = fixture.adapter.cachedTableRecords.values.single().getJSONArray("cells")
            fun point(index: Int): JSONObject {
                val opening = cells.getJSONObject(index).getInt("sourcePos")
                return JSONObject().put("kind", "scalar").put("offset",
                    requireNotNull(fixture.adapter.scalarPositionForDoc(opening + 2)))
            }
            val selection = JSONObject().put("type", "cell")
                .put("anchorCell", point(0)).put("headCell", point(1))
            val result = fixture.adapter.callWithEnvelope(JSONObject().put("selection", selection)) {
                UniffiEditorV2Backend.setSelection(fixture.adapter.editorId, it)
            }
            assertTrue(result is EditorV2CallResult.Ok)
            val root = fixture.editor.richTextView.editorEditText
            assertTrue(root.applyUpdateJSON(requireNotNull(fixture.adapter.refreshFromRustState(null))))
            assertSame(root, fixture.editor.richTextView.activeTextInput)
            assertTrue(root.hasFocus())
            assertTrue(root.authoritativeCellSelectionActive)
            assertFalse(root.isCursorVisible)
            assertFalse(stale.beginBatchEdit())
            stale.commitText("stale", 1)
            assertEquals(before, fixture.adapter.documentJson())
            assertEquals(revision, fixture.adapter.baseDocumentRevision)
        }
        fixture.awaitCommittedFrame()
        fixture.captureScreenshot("native-device-table-cell-selection.png")
        fixture.tapCell(1)
        fixture.onActivity {
            val input = fixture.cellInput()
            assertEquals("Owner", input.text.toString())
            assertEquals("text", JSONObject(requireNotNull(fixture.adapter.cachedAtomicRenderJson))
                .getJSONObject("selection").getString("type"))
            assertFalse(fixture.editor.richTextView.editorEditText.authoritativeCellSelectionActive)
            input.setSelection(input.text.length)
            assertTrue(requireNotNull(input.onCreateInputConnection(EditorInfo())).commitText("!", 1))
            assertEquals(fixture.diagnostics(), listOf("Alpha", "Owner!"), fixture.cellTexts())
        }
    }

    @Test
    @SdkSuppress(minSdkVersion = 29)
    fun tappedCellCommitsTextAndEmojiThroughExpoOwner() = withEditor { fixture ->
        fixture.tapCell(0)
        var expectedRevision = ""
        fixture.onActivity {
            val input = fixture.cellInput()
            assertEquals("Alpha", input.text.toString())
            assertTrue(fixture.editor.hasTableRootNativeOwnerAuthority(fixture.adapter))
            val before = fixture.adapter.baseDocumentRevision
            input.setSelection(input.text.length)
            val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(connection.commitText("X😀", 1))
            assertEquals(fixture.diagnostics(), listOf("AlphaX😀", "Owner"), fixture.cellTexts())
            assertEquals(listOf("Before table.", "After table."), fixture.proseTexts())
            assertEquals(before + 1uL, fixture.adapter.baseDocumentRevision)
            expectedRevision = fixture.adapter.baseDocumentRevision.toString()
        }
        fixture.awaitUpdate(expectedRevision)
        fixture.awaitCommittedFrame()
        fixture.captureScreenshot("native-device-table-expo-edited.png")
    }

    @Test
    fun switchingCellsRetiresOldConnectionAndCommitsCompositionOnce() = withEditor { fixture ->
        fixture.tapCell(0)
        lateinit var input: EditorEditText
        lateinit var oldConnection: InputConnection
        fixture.onActivity {
            input = fixture.cellInput()
            input.setSelection(input.text.length)
            oldConnection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(oldConnection.setComposingText("pending", 1))
            assertEquals(listOf("Alpha", "Owner"), fixture.cellTexts())
        }
        fixture.tapCell(1)
        fixture.onActivity {
            assertSame(input, fixture.cellInput())
            assertEquals(fixture.diagnostics(), listOf("Alphapending", "Owner"), fixture.cellTexts())
            assertEquals("Owner", input.text.toString())
            val beforeStale = fixture.adapter.documentJson()
            assertFalse(oldConnection.beginBatchEdit())
            oldConnection.commitText("stale", 1)
            assertEquals(beforeStale, fixture.adapter.documentJson())
            input.setSelection(input.text.length)
            val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(connection.commitText("!", 1))
            assertEquals(listOf("Alphapending", "Owner!"), fixture.cellTexts())
            assertEquals(listOf("Before table.", "After table."), fixture.proseTexts())
        }
    }

    @Test
    fun hardwareTabMovesToNextCellAndAppendsOneRow() = withEditor { fixture ->
        fixture.tapCell(0)
        fixture.onActivity {
            val input = fixture.cellInput()
            val oldConnection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(input.dispatchKeyEvent(KeyEvent(100L, 100L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_TAB, 0)))
            assertSame(input, fixture.cellInput())
            assertEquals("Owner", input.text.toString())
            assertFalse(oldConnection.beginBatchEdit())
            val before = fixture.adapter.baseDocumentRevision
            assertTrue(input.dispatchKeyEvent(KeyEvent(200L, 200L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_TAB, 0)))
            assertEquals(before + 1uL, fixture.adapter.baseDocumentRevision)
            assertSame(input, fixture.cellInput())
            assertEquals("\u200B", input.text.toString())
            val freshConnection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(input.canDispatchTableCellMutation())
            assertTrue(freshConnection.commitText("N", 1))
            assertEquals(2, fixture.tableRowCount())
            assertEquals("N", fixture.secondRowFirstCellText())
            assertEquals(listOf("Alpha", "Owner"), fixture.cellTexts())
            assertEquals(listOf("Before table.", "After table."), fixture.proseTexts())
        }
    }

    @Test
    fun hardwareRightArrowCrossesCellsThenEntersFollowingProse() = withEditor { fixture ->
        fixture.tapCell(0)
        fixture.onActivity {
            val input = fixture.cellInput()
            input.setSelection(input.text.length)
            val oldConnection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            val revision = fixture.adapter.baseDocumentRevision
            assertTrue(input.dispatchKeyEvent(KeyEvent(310L, 310L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_RIGHT, 0)))
            assertSame(input, fixture.cellInput())
            assertEquals("Owner", input.text.toString())
            assertFalse(oldConnection.beginBatchEdit())
            assertEquals(revision, fixture.adapter.baseDocumentRevision)

            input.setSelection(input.text.length)
            assertTrue(input.dispatchKeyEvent(KeyEvent(311L, 311L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_RIGHT, 0)))
            val root = fixture.editor.richTextView.editorEditText
            assertSame(root, fixture.editor.richTextView.activeTextInput)
            assertEquals(root.text.toString().indexOf("After table."), root.selectionStart)
            assertEquals(revision, fixture.adapter.baseDocumentRevision)
            assertTrue(requireNotNull(root.onCreateInputConnection(EditorInfo()))
                .commitText("X", 1))
            assertEquals(listOf("Before table.", "XAfter table."), fixture.proseTexts())
            assertEquals(1, fixture.tableRowCount())
        }
    }

    @Test
    fun hardwareRightArrowEntersRtlTextAtVisualLeftEdge() =
        withEditor(DOCUMENT.replace("\"Owner\"", "\"אבג\"")) { fixture ->
            fixture.tapCell(0)
            fixture.onActivity {
                val input = fixture.cellInput()
                input.setSelection(input.text.length)
                val revision = fixture.adapter.baseDocumentRevision
                assertTrue(input.dispatchKeyEvent(KeyEvent(320L, 320L,
                    KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_RIGHT, 0)))
                assertSame(input, fixture.cellInput())
                assertEquals("אבג", input.text.toString())
                val entry = input.selectionStart
                assertEquals(entry, input.layout.getOffsetToLeftOf(entry))
                val nativeNext = input.layout.getOffsetToRightOf(entry)
                assertTrue(nativeNext != entry)
                assertTrue(input.dispatchKeyEvent(KeyEvent(321L, 321L,
                    KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_RIGHT, 0)))
                assertSame(input, fixture.cellInput())
                assertEquals("אבג", input.text.toString())
                assertEquals(nativeNext, input.selectionStart)
                assertEquals(revision, fixture.adapter.baseDocumentRevision)
            }
        }

    @Test
    @SdkSuppress(minSdkVersion = 29)
    fun nestedTableRendersReadOnlyWhileHardwareTabSkipsItsOuterCell() = withEditor(NESTED_DOCUMENT) { fixture ->
        fixture.onActivity {
            assertTrue(fixture.nestedContentRendered())
            assertEquals(listOf("Before table.", "After table."), fixture.proseTexts())
        }
        fixture.awaitCommittedFrame()
        fixture.captureScreenshot("native-device-nested-table-mounted.png")
        fixture.tapCell(1)
        fixture.onActivity {
            assertSame(fixture.editor.richTextView.editorEditText,
                fixture.editor.richTextView.activeTextInput)
        }
        fixture.tapCell(0)
        fixture.onActivity {
            val input = fixture.cellInput()
            assertEquals("Alpha", input.text.toString())
            assertTrue(input.dispatchKeyEvent(KeyEvent(100L, 100L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_TAB, 0)))
            assertEquals("Owner", input.text.toString())
            input.setSelection(input.text.length)
            assertTrue(requireNotNull(input.onCreateInputConnection(EditorInfo())).commitText("!", 1))
            assertEquals("Owner!", fixture.outerCellText(2))
            assertTrue(input.dispatchKeyEvent(KeyEvent(200L, 200L,
                KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_TAB, 0, KeyEvent.META_SHIFT_ON)))
            assertEquals("Alpha", input.text.toString())
            assertEquals("Nested", fixture.nestedCellText())
            assertEquals(listOf("Before table.", "After table."), fixture.proseTexts())
        }
        fixture.awaitCommittedFrame()
        fixture.captureScreenshot("native-device-nested-table-edited.png")
    }

    @Test
    fun proseAndWrapperBlurFinishCompositionAndReadOnlyRetiresInput() = withEditor { fixture ->
        fixture.tapCell(0)
        fixture.onActivity {
            val input = fixture.cellInput()
            input.setSelection(input.text.length)
            assertTrue(requireNotNull(input.onCreateInputConnection(EditorInfo()))
                .setComposingText("one", 1))
        }
        fixture.tapFollowingProse()
        fixture.onActivity {
            assertSame(fixture.editor.richTextView.editorEditText,
                fixture.editor.richTextView.activeTextInput)
            assertEquals(fixture.diagnostics(), listOf("Alphaone", "Owner"), fixture.cellTexts())
            assertEquals(listOf("Before table.", "After table."), fixture.proseTexts())
        }
        fixture.tapCell(1)
        fixture.onActivity {
            val input = fixture.cellInput()
            input.setSelection(input.text.length)
            val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(connection.setComposingText("two", 1))
            fixture.editor.blur()
            assertEquals(listOf("Alphaone", "Ownertwo"), fixture.cellTexts())
        }
        fixture.onActivity {
            assertFalse(fixture.editor.richTextView.activeTextInput.hasFocus())
            assertEquals(listOf("Alphaone", "Ownertwo"), fixture.cellTexts())
        }
        fixture.tapCell(1)
        fixture.onActivity {
            val input = fixture.cellInput()
            assertTrue(input.isAuthorizedForTableCellInput())
            val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            assertTrue(connection.beginBatchEdit())
            connection.endBatchEdit()
            assertTrue(input.isAuthorizedForTableCellInput())
            fixture.editor.setEditable(false)
            val beforeStale = fixture.adapter.documentJson()
            assertFalse(connection.beginBatchEdit())
            connection.commitText("stale", 1)
            assertEquals(beforeStale, fixture.adapter.documentJson())
            assertEquals(listOf("Before table.", "After table."), fixture.proseTexts())
        }
    }

    private fun withEditor(document: String = DOCUMENT, test: (Fixture) -> Unit) {
        ActivityScenario.launch(NativeEditorOutsideTapActivity::class.java).use { scenario ->
            val editorRef = AtomicReference<NativeEditorExpoView>()
            val created = when (val result = UniffiEditorV2Backend.create(CONFIG, null)) {
                is EditorV2CallResult.Ok -> result.value
                is EditorV2CallResult.Err -> error("create failed: ${result.error.code}: ${result.error.message}")
            }
            val adapter = requireNotNull(EditorV2Adapter.attach(
                UniffiEditorV2Backend, JSONObject(created).getString("editorId"), false
            ))
            val token = EditorV2Registry.register(adapter)
            try {
                requireNotNull(adapter.setContentJson(document))
                val updates = Collections.synchronizedList(mutableListOf<Map<String, Any>>())
                scenario.onActivity { activity ->
                    initializeSoLoaderIfAvailable(activity)
                    val expo = testExpoContext(activity)
                    val root = FrameLayout(activity).apply { setBackgroundColor(Color.WHITE) }
                    val editor = NativeEditorExpoView(expo.first, expo.second).apply {
                        clipToPadding = false
                        setShowToolbar(false)
                        onFocusChangeForTesting = {}
                        onAddonEventForTesting = {}
                        onEditorUpdateForTesting = updates::add
                        onEditorReadyForTesting = {}
                        onSelectionChangeForTesting = {}
                        onContentHeightChangeForTesting = {}
                        onAtomLayoutForTesting = {}
                    }
                    root.addView(editor, FrameLayout.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT, dp(activity, 300)
                    ).apply {
                        topMargin = dp(activity, 48)
                        leftMargin = dp(activity, 16)
                        rightMargin = dp(activity, 16)
                    })
                    activity.setContentView(root)
                    editor.setEditorId(token)
                    editorRef.set(editor)
                }
                val fixture = Fixture(scenario, editorRef, adapter, updates)
                fixture.awaitTableLayout()
                test(fixture)
            } finally {
                scenario.onActivity { editorRef.get()?.setEditorId(0L) }
                releasePairedV2TestEditor(token)
            }
        }
    }

    private inner class Fixture(
        private val scenario: ActivityScenario<NativeEditorOutsideTapActivity>,
        private val editorRef: AtomicReference<NativeEditorExpoView>,
        val adapter: EditorV2Adapter,
        private val updates: List<Map<String, Any>>
    ) {
        val editor: NativeEditorExpoView get() = requireNotNull(editorRef.get())

        fun onActivity(action: () -> Unit) = scenario.onActivity { action() }

        fun cellInput(): EditorEditText {
            val input = editor.richTextView.activeTextInput
            assertTrue("cell input must be active", input !== editor.richTextView.editorEditText)
            assertTrue("cell input must hold focus", input.hasFocus())
            return input
        }

        fun diagnostics(): String {
            val root = editor.richTextView.editorEditText
            val input = editor.richTextView.activeTextInput
            return "owner=${editor.hasTableRootNativeOwnerAuthority(adapter)} " +
                "revision=${adapter.baseDocumentRevision} " +
                "active=${input.text} focused=${input.hasFocus()} " +
                "cell=${input.isTableCellInput} map=${input.tableCellPositionMap?.binding} " +
                "epoch=${adapter.positionEpoch} authority=${input.tableCellInputAuthority?.invoke()} " +
                "dispatch=${input.canDispatchTableCellMutation()} " +
                "generation=${input.inputConnectionGenerationForTesting()} " +
                "activeIc=${input.activeInputConnection} " +
                "inputTrace=${input.imeTraceSnapshotForTesting().takeLast(20)} " +
                "rootTrace=${root.imeTraceSnapshotForTesting().takeLast(20)} " +
                "notes=${adapter.debugNotes.takeLast(10)}"
        }

        fun cellTexts(): List<String> {
            val rows = JSONObject(requireNotNull(adapter.documentJson())).getJSONArray("content")
                .getJSONObject(1).getJSONArray("content").getJSONObject(0)
                .getJSONArray("content")
            return (0 until rows.length()).map { index ->
                rows.getJSONObject(index).getJSONArray("content").getJSONObject(0)
                    .getJSONArray("content").getJSONObject(0).getString("text")
            }
        }

        fun outerCellText(index: Int): String = JSONObject(requireNotNull(adapter.documentJson()))
            .getJSONArray("content").getJSONObject(1).getJSONArray("content").getJSONObject(0)
            .getJSONArray("content").getJSONObject(index).getJSONArray("content")
            .getJSONObject(0).getJSONArray("content").getJSONObject(0).getString("text")

        fun nestedCellText(): String = JSONObject(requireNotNull(adapter.documentJson()))
            .getJSONArray("content").getJSONObject(1).getJSONArray("content").getJSONObject(0)
            .getJSONArray("content").getJSONObject(1).getJSONArray("content").getJSONObject(0)
            .getJSONArray("content").getJSONObject(0).getJSONArray("content").getJSONObject(0)
            .getJSONArray("content").getJSONObject(0)
            .getJSONArray("content").getJSONObject(0).getString("text")

        fun nestedContentRendered(): Boolean {
            val outer = tableHostOrNull()?.preparedLayout?.blocks?.singleOrNull()?.tableSurface
                ?: return false
            val nested = outer.cells.getOrNull(1)?.content?.blocks
                ?.singleOrNull { it.tableSurface != null }?.tableSurface ?: return false
            return nested.cells.singleOrNull()?.content?.blocks?.flatMap { it.fragments }
                ?.any { it.layout?.text?.contains("Nested") == true } == true
        }

        fun tableRowCount(): Int = JSONObject(requireNotNull(adapter.documentJson()))
            .getJSONArray("content").getJSONObject(1).getJSONArray("content").length()

        fun secondRowFirstCellText(): String = JSONObject(requireNotNull(adapter.documentJson()))
            .getJSONArray("content").getJSONObject(1).getJSONArray("content")
            .getJSONObject(1).getJSONArray("content").getJSONObject(0)
            .getJSONArray("content").getJSONObject(0).getJSONArray("content")
            .getJSONObject(0).getString("text")

        fun proseTexts(): List<String> {
            val blocks = JSONObject(requireNotNull(adapter.documentJson())).getJSONArray("content")
            return listOf(0, 2).map { index ->
                blocks.getJSONObject(index).getJSONArray("content").getJSONObject(0).getString("text")
            }
        }

        fun awaitTableLayout() = waitUntil("Expo table layout") {
            var ready = false
            scenario.onActivity {
                val view = editor.richTextView
                val host = tableHostOrNull()
                val input = view.editorEditText
                ready = host?.preparedLayout?.blocks?.any { it.tableSurface != null } == true &&
                    view.width > 0 && host.width > 0 && input.layout != null &&
                    editor.hasTableRootNativeOwnerAuthority(adapter)
            }
            ready
        }

        fun awaitUpdate(expectedRevision: String) = waitUntil("wrapper editor update event") {
            var received = false
            scenario.onActivity {
                received = updates.any { event ->
                    event["editorId"] == adapter.editorId &&
                        event["documentRevision"] == expectedRevision
                }
            }
            received
        }

        fun tapCell(index: Int) {
            instrumentation.waitForIdleSync()
            var x = 0f
            var y = 0f
            scenario.onActivity {
                val host = requireNotNull(tableHostOrNull())
                val block = requireNotNull(host.preparedLayout).blocks.single { it.tableSurface != null }
                val surface = requireNotNull(block.tableSurface)
                val source = requireNotNull(surface.sourceTable).cells[index].sourcePos.toInt()
                val frame = surface.cells.single { it.sourcePosition == source }.frame
                val bounds = requireNotNull(block.tableBounds)
                val location = IntArray(2)
                host.getLocationOnScreen(location)
                x = location[0] + bounds.left + frame.left + frame.width / 2f
                y = location[1] + bounds.top + frame.top + frame.height / 2f
            }
            tap(x, y)
        }

        fun tapFollowingProse() {
            instrumentation.waitForIdleSync()
            var x = 0f
            var y = 0f
            scenario.onActivity {
                val input = editor.richTextView.editorEditText
                val offset = input.text.indexOf("After table.") + 4
                assertTrue(offset >= 4)
                val layout = requireNotNull(input.layout)
                val line = layout.getLineForOffset(offset)
                val location = IntArray(2)
                input.getLocationOnScreen(location)
                x = location[0] + input.totalPaddingLeft + layout.getPrimaryHorizontal(offset)
                y = location[1] + input.totalPaddingTop +
                    (layout.getLineTop(line) + layout.getLineBottom(line)) / 2f
            }
            tap(x, y)
        }

        fun awaitCommittedFrame() {
            val committed = CountDownLatch(1)
            val callback = Runnable { committed.countDown() }
            scenario.onActivity {
                val decor = it.window.decorView
                assertTrue(decor.isHardwareAccelerated)
                decor.viewTreeObserver.registerFrameCommitCallback(callback)
                decor.invalidate()
            }
            try {
                assertTrue("edited Expo cell frame must be submitted",
                    committed.await(3, TimeUnit.SECONDS))
            } finally {
                scenario.onActivity {
                    it.window.decorView.viewTreeObserver.unregisterFrameCommitCallback(callback)
                }
            }
        }

        fun captureScreenshot(filename: String) {
            instrumentation.waitForIdleSync()
            val directory = requireNotNull(instrumentation.targetContext.getExternalFilesDir(null))
            val bitmap = requireNotNull(instrumentation.uiAutomation.takeScreenshot())
            try {
                java.io.File(directory, filename).outputStream().use { output ->
                    assertTrue(bitmap.compress(Bitmap.CompressFormat.PNG, 100, output))
                }
            } finally {
                bitmap.recycle()
            }
        }

        private fun tableHostOrNull(): PreparedProseDrawingView? {
            val frame = editor.richTextView.editorContentFrame
            return (0 until frame.childCount).map(frame::getChildAt)
                .filterIsInstance<PreparedProseDrawingView>().singleOrNull()
        }
    }

    private fun tap(x: Float, y: Float) {
        val start = SystemClock.uptimeMillis()
        val down = MotionEvent.obtain(start, start, MotionEvent.ACTION_DOWN, x, y, 0)
        val up = MotionEvent.obtain(start, start + 40, MotionEvent.ACTION_UP, x, y, 0)
        try {
            instrumentation.sendPointerSync(down)
            instrumentation.sendPointerSync(up)
            instrumentation.waitForIdleSync()
        } finally {
            down.recycle()
            up.recycle()
        }
    }

    private fun waitUntil(description: String, condition: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + 5_000L
        do {
            instrumentation.waitForIdleSync()
            if (condition()) return
            SystemClock.sleep(20)
        } while (SystemClock.uptimeMillis() < deadline)
        assertTrue(description, condition())
    }

    private fun dp(context: Context, value: Int): Int =
        (value * context.resources.displayMetrics.density).toInt()

    private fun initializeSoLoaderIfAvailable(context: Context) {
        try {
            Class.forName("com.facebook.soloader.SoLoader")
                .getMethod("init", Context::class.java, Boolean::class.javaPrimitiveType)
                .invoke(null, context, false)
        } catch (_: Throwable) {
        }
    }

    private fun testExpoContext(activity: Activity): Pair<Context, AppContext> {
        val reactContext = Class.forName("com.facebook.react.bridge.BridgeReactContext")
            .getConstructor(Context::class.java).newInstance(activity) as Context
        reactContext.javaClass.getMethod("onHostResume", Activity::class.java)
            .invoke(reactContext, activity)
        val provider = object : ModulesProvider {
            override fun getModulesMap(): Map<Class<out Module>, String?> = emptyMap()
        }
        val constructor = AppContext::class.java.constructors.first {
            it.parameterTypes.size == 3
        }
        val context = constructor.newInstance(provider,
            ModuleRegistry(emptyList(), emptyList()), WeakReference(reactContext)) as AppContext
        return reactContext to context
    }

    companion object {
        private const val CONFIG = """{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"""
        private const val DOCUMENT = """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Before table."}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Alpha"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Owner"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"After table."}]}]}"""
        private const val NESTED_DOCUMENT = """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Before table."}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Alpha"}]}]},{"type":"table_cell","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Nested"}]}]}]}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Owner"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"After table."}]}]}"""
    }
}
