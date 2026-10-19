package com.apollohg.editor.surface

import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Rect
import android.graphics.RectF
import android.graphics.drawable.ColorDrawable
import android.os.Build
import android.os.SystemClock
import android.text.Spanned
import android.text.TextPaint
import android.text.style.CharacterStyle
import android.text.style.ForegroundColorSpan
import android.text.style.ReplacementSpan
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.ViewGroup
import android.view.ViewTreeObserver
import android.view.animation.AlphaAnimation
import android.view.animation.Animation
import android.view.animation.LayoutAnimationController
import android.view.animation.TranslateAnimation
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.ExtractedTextRequest
import android.widget.FrameLayout
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.SdkSuppress
import androidx.test.platform.app.InstrumentationRegistry
import com.apollohg.editor.AtomBlockSpan
import com.apollohg.editor.AtomRenderConfiguration
import com.apollohg.editor.EDITOR_PUBLIC_THEME_FIXTURES_ASSET
import com.apollohg.editor.EditorInputConnection
import com.apollohg.editor.EditorResolvedTextSpan
import com.apollohg.editor.EditorTextStyle
import com.apollohg.editor.PositionBridge
import com.apollohg.editor.TransientComposingTextStyleSpan
import com.apollohg.editor.applySelectionFromJSON
import com.apollohg.editor.runOnMainSyncWithResult
import com.apollohg.editor.saveDeviceScreenshot
import java.io.BufferedReader
import java.io.Closeable
import java.io.File
import java.io.IOException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlin.math.ceil
import kotlin.math.floor
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
@SdkSuppress(minSdkVersion = Build.VERSION_CODES.Q)
class EditorSurfaceCaretRenderingTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()

    @Test
    fun scrollViewportRecordsVisibleGlyphsAndKeepsScrolledAndChangedGlyphsPainted() =
        withEditor { activity ->
            val corpus =
                JSONObject(
                    instrumentation.context.assets
                        .open(EDITOR_PUBLIC_THEME_FIXTURES_ASSET)
                        .bufferedReader()
                        .use(BufferedReader::readText)
                )
            val fixtures = corpus.getJSONArray("fixtures")
            val fixture =
                (0 until fixtures.length())
                    .map(fixtures::getJSONObject)
                    .single { it.getString("id") == GLYPH_RECORDING_FIXTURE }
            val glyphs =
                instrumentation.runOnMainSyncWithResult {
                    val update =
                        requireNotNull(activity.adapter.setContentHtml(fixture.getString("html")))
                    assertTrue(
                        "The real corpus update must be accepted",
                        activity.editor.applyUpdateJSON(update)
                    )
                    val input = activity.editor
                    val last = input.text.indexOfLast { !it.isWhitespace() }
                    assertTrue(
                        "The real corpus must contain a distant nonempty glyph",
                        last > CARET_OFFSET
                    )
                    val firstGlyph = RecordingGlyph()
                    val lastGlyph = RecordingGlyph()
                    input.text.setSpan(firstGlyph, 0, 1, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                    input.text.setSpan(lastGlyph, last, last + 1, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                    input.setSelection(CARET_OFFSET)
                    activity.richTextView.editorScrollView.scrollTo(0, 0)
                    Triple(firstGlyph, lastGlyph, last)
                }
            capture(activity).use { frame ->
                assertTrue(
                    "The first visible custom glyph must actually paint",
                    frame.hasColor(GLYPH_COLOR)
                )
            }
            instrumentation.runOnMainSyncWithResult {
                val input = activity.editor
                val visible = Rect()
                assertTrue(input.getLocalVisibleRect(visible))
                val distantLine = input.layout.getLineForOffset(glyphs.third)
                assertTrue(
                    "The final corpus glyph must be outside the actual native viewport",
                    input.layout.getLineTop(
                        distantLine
                    ) + input.compoundPaddingTop > visible.bottom
                )
            }
            assertTrue(
                "Visible text must be recorded on the hardware canvas",
                glyphs.first.hardwareDraws.get() > 0
            )
            assertEquals(
                "Offscreen text must not allocate hardware glyph records before its viewport is displayed",
                0,
                glyphs.second.hardwareDraws.get()
            )
            val firstRecords = glyphs.first.hardwareDraws.get()
            instrumentation.runOnMainSyncWithResult {
                val input = activity.editor
                activity.richTextView.editorScrollView.scrollTo(
                    0,
                    input.layout.getLineTop(input.layout.getLineForOffset(glyphs.third))
                )
            }
            capture(activity).use { frame ->
                assertTrue(
                    "Scrolling must display the distant actual glyph",
                    frame.hasColor(GLYPH_COLOR)
                )
            }
            assertTrue(
                "The newly visible glyph must now be recorded",
                glyphs.second.hardwareDraws.get() > 0
            )
            assertEquals(
                "The hidden first glyph must stay outside the new recording",
                firstRecords,
                glyphs.first.hardwareDraws.get()
            )
            instrumentation.runOnMainSyncWithResult {
                glyphs.second.color = CHANGED_GLYPH_COLOR
                activity.editor.invalidate()
            }
            capture(activity).use { frame ->
                assertTrue(
                    "Invalidating a visible mutable span must repaint its actual color",
                    frame.hasColor(CHANGED_GLYPH_COLOR)
                )
            }
            instrumentation.runOnMainSyncWithResult {
                val input = activity.editor
                input.highlightColor = VIEWPORT_SELECTION_COLOR
                input.setSelection(glyphs.third - SELECTION_RANGE_DISTANCE, glyphs.third + 1)
                input.scrollTo(OWN_SCROLL_X, OWN_SCROLL_Y)
            }
            capture(activity).use { frame ->
                assertTrue(
                    "Selection with own and ancestor scrolling must retain the distant glyph",
                    frame.hasColor(CHANGED_GLYPH_COLOR)
                )
                assertTrue(
                    "The actual noncollapsed selection must paint inside the clipped viewport",
                    frame.hasColor(VIEWPORT_SELECTION_COLOR)
                )
            }
            instrumentation.runOnMainSyncWithResult {
                activity.editor.scrollTo(0, 0)
                activity.editor.setSelection(CARET_OFFSET)
                activity.richTextView.editorScrollView.scrollTo(0, 0)
            }
            capture(activity).use { frame ->
                assertTrue(
                    "Returning to the first viewport must restore its glyph pixels",
                    frame.hasColor(GLYPH_COLOR)
                )
            }
            val beforeTransform = glyphs.second.hardwareDraws.get()
            instrumentation.runOnMainSyncWithResult {
                activity.richTextView.editorContentFrame.scaleX = GLYPH_FALLBACK_SCALE
            }
            capture(activity).use { frame ->
                assertTrue(
                    "A transformed ancestor must retain the visible glyph pixels",
                    frame.hasColor(GLYPH_COLOR)
                )
            }
            assertTrue(
                "A transformed ancestor must use the full hardware recording fallback",
                glyphs.second.hardwareDraws.get() > beforeTransform
            )
            val beforeClippingReturn = glyphs.first.hardwareDraws.get()
            instrumentation.runOnMainSyncWithResult {
                activity.richTextView.editorContentFrame.scaleX = 1f
            }
            capture(activity).close()
            assertTrue(
                "Returning to identity must automatically replace the fallback glyph recording",
                glyphs.first.hardwareDraws.get() > beforeClippingReturn
            )
            val afterTransform = glyphs.second.hardwareDraws.get()
            instrumentation.runOnMainSyncWithResult { activity.editor.invalidate() }
            capture(activity).use { frame ->
                assertTrue(
                    "Returning from the transform must preserve the visible glyph",
                    frame.hasColor(GLYPH_COLOR)
                )
            }
            assertEquals(
                "Returning from the transform must stop recording the offscreen glyph",
                afterTransform,
                glyphs.second.hardwareDraws.get()
            )
            instrumentation.runOnMainSyncWithResult {
                val input = activity.editor
                val targetTop =
                    input.layout.getLineTop(input.layout.getLineForOffset(glyphs.third)) +
                        input.compoundPaddingTop
                val bitmap = Bitmap.createBitmap(
                    input.width,
                    activity.richTextView.editorViewport.height,
                    Bitmap.Config.ARGB_8888
                )
                Frame(bitmap, Rect(), input.caretColor, JSONObject()).use { frame ->
                    val canvas = Canvas(bitmap)
                    canvas.translate(0f, -targetTop.toFloat())
                    input.draw(canvas)
                    assertTrue(
                        "The software canvas must draw the distant glyph",
                        glyphs.second.softwareDraws.get() > 0
                    )
                    assertTrue(
                        "Software drawing must honor its actual canvas clip rather than the hardware viewport",
                        frame.hasColor(CHANGED_GLYPH_COLOR)
                    )
                }
            }
        }

    @Test
    fun fullyClippedEditorSettlesAndReentryAndReattachmentPaintItsGlyph() = withEditor { activity ->
        val glyph = installGlyph(activity)
        capture(activity).close()
        val preDraws = AtomicInteger()
        val observer =
            ViewTreeObserver.OnPreDrawListener {
                preDraws.incrementAndGet()
                true
            }
        val originalParams = instrumentation.runOnMainSyncWithResult {
            activity.editor.layoutParams
        }
        val root =
            instrumentation.runOnMainSyncWithResult {
                val rich = activity.richTextView
                val input = activity.editor
                input.isCursorVisible = false
                input.clearFocus()
                rich.editorContentFrame.removeView(input)
                FrameLayout(activity).also { parent ->
                    parent.clipChildren = true
                    parent.clipToPadding = true
                    rich.editorViewport.addView(
                        parent,
                        FrameLayout.LayoutParams(
                            ViewGroup.LayoutParams.MATCH_PARENT,
                            ViewGroup.LayoutParams.MATCH_PARENT
                        )
                    )
                    parent.addView(
                        input,
                        FrameLayout.LayoutParams(
                            ViewGroup.LayoutParams.MATCH_PARENT,
                            input.height
                        ).apply {
                            topMargin = rich.editorViewport.height * HIDDEN_VIEWPORT_DISTANCE
                        }
                    )
                    parent.viewTreeObserver.addOnPreDrawListener(observer)
                }
            }
        try {
            instrumentation.waitForIdleSync()
            instrumentation.runOnMainSyncWithResult {
                val input = activity.editor
                assertTrue(
                    "The hidden editor must remain attached to the real hardware window",
                    input.isAttachedToWindow && input.isHardwareAccelerated
                )
                assertTrue(
                    "The hidden editor must retain identity transforms",
                    input.matrix.isIdentity && root.matrix.isIdentity
                )
                assertFalse(
                    "The editor must be entirely outside the real clipped viewport",
                    input.getLocalVisibleRect(Rect())
                )
            }
            SystemClock.sleep(VIEWPORT_SETTLE_WINDOW_MS)
            val beforeIdle = preDraws.get()
            assertTrue(
                "The observer must have seen actual hardware-window traversals",
                beforeIdle > 0
            )
            val beforeRecords = glyph.hardwareDraws.get()
            SystemClock.sleep(VIEWPORT_SETTLE_WINDOW_MS)
            assertTrue(
                "A fully clipped editor must settle instead of continuously scheduling frames: before=$beforeIdle after=${preDraws.get()}",
                preDraws.get() - beforeIdle <= IDLE_FRAME_BUDGET
            )
            assertEquals(
                "A settled hidden editor must not record its glyph",
                beforeRecords,
                glyph.hardwareDraws.get()
            )
            instrumentation.runOnMainSyncWithResult {
                val input = activity.editor
                input.layoutParams =
                    (input.layoutParams as FrameLayout.LayoutParams).apply { topMargin = 0 }
            }
            capture(activity).use { frame ->
                val diagnostics =
                    instrumentation.runOnMainSyncWithResult {
                        val input = activity.editor
                        val visible = Rect().also(input::getLocalVisibleRect)
                        "span=${input.text.getSpanStart(
                            glyph
                        )} records=${glyph.hardwareDraws.get()} " +
                            "visible=$visible " +
                            "scroll=${activity.richTextView.editorScrollView.scrollY} " +
                            "text=${input.text.take(CARET_OFFSET)} frame=${frame.diagnostics}"
                    }
                if (!frame.hasColor(
                        GLYPH_COLOR
                    )
                ) {
                    instrumentation.saveDeviceScreenshot(
                        "viewport-reentry-failure.png",
                        frame.pixels
                    )
                }
                assertTrue(
                    "Reentering the viewport must paint the existing glyph: $diagnostics",
                    frame.hasColor(GLYPH_COLOR)
                )
            }
            assertTrue(
                "Reentry must record the newly visible glyph",
                glyph.hardwareDraws.get() > beforeRecords
            )
            val beforeReattach = glyph.hardwareDraws.get()
            instrumentation.runOnMainSyncWithResult {
                val input = activity.editor
                val params = input.layoutParams
                root.removeView(input)
                root.addView(input, params)
            }
            capture(activity).use { frame ->
                assertTrue(
                    "Reattachment must paint the glyph using the new window observation",
                    frame.hasColor(GLYPH_COLOR)
                )
            }
            assertTrue(
                "Reattachment must record the actual glyph",
                glyph.hardwareDraws.get() > beforeReattach
            )
        } finally {
            instrumentation.runOnMainSyncWithResult {
                root.viewTreeObserver.takeIf { it.isAlive }?.removeOnPreDrawListener(observer)
                root.removeView(activity.editor)
                activity.richTextView.editorViewport.removeView(root)
                activity.richTextView.editorContentFrame.addView(
                    activity.editor,
                    originalParams
                )
            }
        }
    }

    @Test
    fun naturalCaretBlinkReusesGlyphRecordingAndSoftwareDrawKeepsOneCaret() =
        withEditor { activity ->
            val glyph = installGlyph(activity)
            capture(activity).close()
            val beforeBlink = glyph.hardwareDraws.get()
            assertTrue(
                "The visible replacement glyph must actually be hardware recorded",
                beforeBlink > 0
            )
            withBlinkFrames(activity) { visible, hidden ->
                assertOutsideCaretEqual(visible, hidden)
                assertTrue(
                    "The genuine replacement glyph must remain visible",
                    visible.hasColor(GLYPH_COLOR)
                )
            }
            assertEquals(
                "Natural caret blink must reuse the already recorded document glyph commands",
                beforeBlink,
                glyph.hardwareDraws.get()
            )

            instrumentation.runOnMainSyncWithResult {
                val input = activity.editor
                input.setSelection(CARET_OFFSET + 1)
                input.setSelection(CARET_OFFSET)
                assertTrue(input.hasFocus() && input.hasWindowFocus())
                val expected = Bitmap.createBitmap(
                    input.width,
                    input.height,
                    Bitmap.Config.ARGB_8888
                )
                val actual = Bitmap.createBitmap(input.width, input.height, Bitmap.Config.ARGB_8888)
                try {
                    input.draw(Canvas(expected))
                    activity.richTextView.editorContentFrame.draw(Canvas(actual))
                    assertTrue(
                        "Mounted hardware views drawn to a software Canvas must paint exactly one inline caret",
                        expected.sameAs(actual)
                    )
                    assertTrue(
                        "Software control must draw the actual glyph",
                        glyph.softwareDraws.get() > 0
                    )
                } finally {
                    expected.recycle()
                    actual.recycle()
                }
            }

            instrumentation.runOnMainSyncWithResult {
                val input = activity.editor
                val layerBytes = input.width.toLong() * input.height * ARGB_8888_BYTES_PER_PIXEL
                val layerLimit = ViewConfiguration.get(input.context).scaledMaximumDrawingCacheSize
                assertTrue(
                    "A ${input.width}x${input.height} software layer needs $layerBytes bytes " +
                        "but the platform only draws software layers up to $layerLimit bytes",
                    layerBytes <= layerLimit
                )
                input.setLayerType(View.LAYER_TYPE_SOFTWARE, null)
            }
            capture(activity).close()
            val beforeSoftwareBlink = glyph.softwareDraws.get()
            withBlinkFrames(activity) { visible, hidden ->
                assertOutsideCaretEqual(visible, hidden)
            }
            assertTrue(
                "An actual software layer must repaint its inline glyph and caret with natural blink",
                glyph.softwareDraws.get() > beforeSoftwareBlink
            )
            instrumentation.runOnMainSyncWithResult {
                activity.editor.setLayerType(View.LAYER_TYPE_NONE, null)
            }
            capture(activity).close()
            val afterHardwareReturn = glyph.hardwareDraws.get()
            withBlinkFrames(activity) { visible, hidden ->
                assertOutsideCaretEqual(visible, hidden)
            }
            assertEquals(
                "Returning from software to hardware must remove any baked caret and resume body reuse",
                afterHardwareReturn,
                glyph.hardwareDraws.get()
            )

            val beforeMutation = glyph.hardwareDraws.get()
            instrumentation.runOnMainSyncWithResult {
                activity.editor.text.setSpan(
                    ForegroundColorSpan(Color.MAGENTA),
                    CARET_OFFSET,
                    CARET_OFFSET + 1,
                    Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                )
            }
            capture(activity).close()
            assertTrue(
                "A real live text-span mutation must invalidate the body recording",
                glyph.hardwareDraws.get() > beforeMutation
            )
            val originalHtml = instrumentation.runOnMainSyncWithResult {
                activity.adapter.documentHtml()
            }
            instrumentation.runOnMainSyncWithResult {
                val connection =
                    requireNotNull(activity.editor.onCreateInputConnection(EditorInfo()))
                assertTrue(connection.commitText(NATIVE_EDIT, 1))
                assertTrue(requireNotNull(activity.adapter.documentHtml()).contains(NATIVE_EDIT))
                assertTrue(connection.deleteSurroundingText(NATIVE_EDIT.length, 0))
                assertEquals(originalHtml, activity.adapter.documentHtml())
                connection.closeConnection()
            }
            withBlinkFrames(activity) { visible, hidden ->
                assertOutsideCaretEqual(visible, hidden)
            }
        }

    @Test
    fun postCaretDecorationVisibilityAndReparentingPreserveFallbackAndClearOldPixels() =
        withEditor { activity ->
            val glyph = installGlyph(activity)
            val decoration = EditorResolvedTextSpan(
                EditorTextStyle(
                    textDecorationLine = DECORATION_LINE,
                    textDecorationColor = DECORATION_COLOR,
                    textDecorationStyle = DECORATION_STYLE
                ),
                instrumentation.runOnMainSyncWithResult {
                    activity.resources.displayMetrics.density
                }
            )
            instrumentation.runOnMainSyncWithResult {
                activity.editor.text.setSpan(
                    decoration,
                    0,
                    FIRST_PARAGRAPH_END,
                    Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                )
            }
            capture(activity).close()
            val beforeFallbackBlink = glyph.hardwareDraws.get()
            withBlinkFrames(activity) { visible, hidden ->
                assertOutsideCaretEqual(visible, hidden)
                val point = instrumentation.runOnMainSyncWithResult {
                    val input = activity.editor
                    val layout = input.layout
                    val line = layout.getLineForOffset(input.selectionEnd)
                    val paint = android.text.TextPaint(layout.paint)
                    decoration.updateMeasureState(paint)
                    val strokeWidth = maxOf(1f, paint.textSize / 16f)
                    val y = layout.getLineBaseline(line) +
                        maxOf(strokeWidth, paint.fontMetrics.descent * .5f)
                    val inputLocation = IntArray(2).also(input::getLocationOnScreen)
                    val viewportLocation = IntArray(
                        2
                    ).also(activity.richTextView.editorViewport::getLocationOnScreen)
                    visible.caret.centerX() to (
                        inputLocation[1] - viewportLocation[1] + input.compoundPaddingTop -
                            input.scrollY + y
                        ).toInt()
                }
                assertTrue(
                    "Post-caret underline must cover the caret at its genuine stroke intersection",
                    closeColor(visible.pixels.getPixel(point.first, point.second), DECORATION_COLOR)
                )
            }
            assertTrue(
                "The decoration fallback must continue to repaint the real glyph with natural blink",
                glyph.hardwareDraws.get() > beforeFallbackBlink
            )
            instrumentation.runOnMainSyncWithResult { activity.editor.text.removeSpan(decoration) }
            capture(activity).close()
            val beforeReuse = glyph.hardwareDraws.get()
            withBlinkFrames(activity) { visible, hidden ->
                assertOutsideCaretEqual(visible, hidden)
            }
            assertEquals(
                "Removing decoration must return to body reuse",
                beforeReuse,
                glyph.hardwareDraws.get()
            )

            val oldCaret = capture(activity).use { Rect(it.caret) }
            instrumentation.runOnMainSyncWithResult { activity.editor.visibility = View.INVISIBLE }
            capture(activity, oldCaret).use {
                assertFalse("An invisible input must leave no sibling caret behind", it.hasCaret())
                assertFalse("Its genuine glyph must also be invisible", it.hasColor(GLYPH_COLOR))
            }
            instrumentation.runOnMainSyncWithResult {
                activity.editor.visibility = View.VISIBLE
                activity.editor.requestFocus()
            }
            withBlinkFrames(activity) { visible, hidden ->
                assertOutsideCaretEqual(visible, hidden)
            }

            val standalone = FrameLayout(activity)
            val originalParams = instrumentation.runOnMainSyncWithResult {
                activity.editor.layoutParams
            }
            try {
                instrumentation.runOnMainSyncWithResult {
                    val input = activity.editor
                    activity.richTextView.editorContentFrame.removeView(input)
                    activity.richTextView.editorViewport.addView(
                        standalone,
                        FrameLayout.LayoutParams(
                            ViewGroup.LayoutParams.MATCH_PARENT,
                            ViewGroup.LayoutParams.MATCH_PARENT
                        )
                    )
                    standalone.addView(
                        input,
                        FrameLayout.LayoutParams(
                            ViewGroup.LayoutParams.MATCH_PARENT,
                            ViewGroup.LayoutParams.MATCH_PARENT
                        )
                    )
                    input.requestFocus()
                    input.setSelection(CARET_OFFSET + 1)
                    input.setSelection(CARET_OFFSET)
                }
                capture(activity).close()
                val beforeStandaloneBlink = glyph.hardwareDraws.get()
                withBlinkFrames(activity) { visible, hidden ->
                    assertOutsideCaretEqual(visible, hidden)
                }
                assertTrue(
                    "A reparented standalone input must keep the full inline blink fallback",
                    glyph.hardwareDraws.get() > beforeStandaloneBlink
                )
            } finally {
                instrumentation.runOnMainSyncWithResult {
                    standalone.removeView(activity.editor)
                    activity.richTextView.editorViewport.removeView(standalone)
                    activity.richTextView.editorContentFrame.addView(
                        activity.editor,
                        0,
                        originalParams
                    )
                    activity.editor.requestFocus()
                    activity.editor.setSelection(CARET_OFFSET + 1)
                    activity.editor.setSelection(CARET_OFFSET)
                }
            }
            capture(activity).close()
            val afterReturn = glyph.hardwareDraws.get()
            withBlinkFrames(activity) { visible, hidden ->
                assertOutsideCaretEqual(visible, hidden)
            }
            assertEquals(
                "Returning to the owned frame must rearm body reuse",
                afterReturn,
                glyph.hardwareDraws.get()
            )
        }

    @Test
    fun genuineAtomSelectionCompositionRollbackAndScrollingKeepTheNativeCaret() =
        withEditor { activity ->
            instrumentation.runOnMainSyncWithResult {
                val input = activity.editor
                assertTrue(
                    activity.richTextView.applyAtomRenderConfiguration(
                        AtomRenderConfiguration(
                            setOf(ATOM_TYPE),
                            mapOf(ATOM_TYPE to ATOM_HEIGHT_DP),
                            emptyMap()
                        )
                    )
                )
                val update =
                    requireNotNull(
                        activity.adapter.setContentJson(
                            JSONObject(ATOM_DOCUMENT).apply {
                                val content = getJSONArray("content")
                                repeat(SCROLL_PARAGRAPH_COUNT) {
                                    content.put(JSONObject(SCROLL_PARAGRAPH))
                                }
                            }.toString()
                        )
                    )
                val html = requireNotNull(activity.adapter.documentHtml())
                val payload = JSONObject(update)
                val elements = payload.optJSONArray("renderElements") ?: JSONArray().apply {
                    val blocks = payload.optJSONArray("renderBlocks")
                        ?: payload.getJSONObject("renderPatch").getJSONArray("renderBlocks")
                    for (blockIndex in 0 until blocks.length()) {
                        val block = blocks.getJSONArray(blockIndex)
                        for (elementIndex in 0 until block.length()) put(block.get(elementIndex))
                    }
                }
                val voidAtoms = (0 until elements.length()).map(elements::getJSONObject).filter {
                    it.optString("type") == ATOM_RENDER_TYPE &&
                        it.optString("nodeType") == ATOM_TYPE
                }
                val coreDiagnostic = "html=$html update=$update elements=$elements"
                assertEquals(
                    "The genuine core frame must contain one registered void atom: $coreDiagnostic",
                    1,
                    voidAtoms.size
                )
                assertTrue(
                    "The atom needs a real document position: $coreDiagnostic",
                    voidAtoms.single().opt("docPos") is Number
                )
                assertTrue(
                    "The genuine document must export its horizontal rule: $coreDiagnostic",
                    html.contains(ATOM_HTML_PREFIX)
                )
                assertTrue(
                    "The actual core update must apply: $coreDiagnostic",
                    input.applyUpdateJSON(update)
                )
                val spans = input.text.getSpans(0, input.text.length, AtomBlockSpan::class.java)
                assertEquals(
                    "The root host must project one real AtomBlockSpan: $coreDiagnostic text=${input.text}",
                    1,
                    spans.size
                )
                val atom = spans.single()
                val start = input.text.getSpanStart(atom)
                val scalar = PositionBridge.utf16ToScalar(start, input.text.toString())
                val synced = requireNotNull(activity.adapter.syncSelection(scalar, scalar)) {
                    "The genuine atom selection must sync: scalar=$scalar $coreDiagnostic"
                }
                val selectionUpdate =
                    synced.refreshedUpdateJson
                        ?: requireNotNull(activity.adapter.recoverNativeRender()) {
                            "The genuine selected core frame must normalize for the view: scalar=$scalar sync=$synced"
                        }
                val selected = JSONObject(selectionUpdate).getJSONObject("selection")
                val selectionDiagnostic = "scalar=$scalar sync=$synced update=$selectionUpdate"
                assertEquals(
                    "The authoritative frame must contain text selection: $selectionDiagnostic",
                    ATOM_SELECTION_TYPE,
                    selected.getString("type")
                )
                assertEquals(
                    "The real frame must retain the requested scalar anchor: $selectionDiagnostic",
                    scalar,
                    selected.getInt("anchorScalar")
                )
                assertEquals(
                    "The real frame must retain the requested scalar head: $selectionDiagnostic",
                    scalar,
                    selected.getInt("headScalar")
                )
                assertEquals(
                    "Sync and frame document anchors must match: $selectionDiagnostic",
                    synced.docAnchor,
                    selected.getInt("anchor")
                )
                assertEquals(
                    "Sync and frame document heads must match: $selectionDiagnostic",
                    synced.docHead,
                    selected.getInt("head")
                )
                val selectedRevision = JSONObject(selectionUpdate).getString("documentVersion")
                assertEquals(
                    "Pure selection must match the already installed genuine atom content revision: $selectionDiagnostic",
                    input.lastAppliedDocumentVersion,
                    selectedRevision
                )
                input.applySelectionFromJSON(selected, selectedRevision)
                assertEquals(
                    "The real core frame must carry the collapsed atom boundary",
                    start,
                    input.selectionStart
                )
                assertEquals(start, input.selectionEnd)
                assertFalse(input.isCursorVisible)
                assertEquals(null, input.nativeCursorDrawRect())
                input.setSelection(input.text.getSpanEnd(atom) + 1)
            }
            withBlinkFrames(activity) { visible, hidden ->
                assertOutsideCaretEqual(visible, hidden)
            }
            val authorized = instrumentation.runOnMainSyncWithResult {
                assertTrue(
                    "Moving off the atom must restore the native cursor gate",
                    activity.editor.isCursorVisible
                )
                Triple(
                    activity.editor.text.toString(),
                    activity.editor.selectionEnd,
                    activity.adapter.documentHtml()
                )
            }
            instrumentation.runOnMainSyncWithResult {
                activity.editor.beginExternalTextComposition(COMPOSITION_SESSION)
                activity.editor.updateExternalTextComposition(COMPOSITION_SESSION, COMPOSITION_TEXT)
                assertTrue(activity.editor.text.toString().contains(COMPOSITION_TEXT))
                assertEquals(
                    "Provisional composition must not publish a core edit",
                    authorized.third,
                    activity.adapter.documentHtml()
                )
            }
            withBlinkFrames(activity) { visible, hidden ->
                assertOutsideCaretEqual(visible, hidden)
            }
            instrumentation.runOnMainSyncWithResult {
                activity.editor.cancelExternalTextComposition(
                    COMPOSITION_SESSION,
                    COMPOSITION_CANCEL_CAUSE
                )
                assertEquals(authorized.first, activity.editor.text.toString())
                assertEquals(authorized.second, activity.editor.selectionStart)
                assertEquals(authorized.second, activity.editor.selectionEnd)
                assertEquals(authorized.third, activity.adapter.documentHtml())
                activity.editor.scrollTo(OWN_SCROLL_X, OWN_SCROLL_Y)
                activity.richTextView.editorScrollView.scrollTo(0, ANCESTOR_SCROLL_Y)
                assertEquals(OWN_SCROLL_X, activity.editor.scrollX)
                assertEquals(OWN_SCROLL_Y, activity.editor.scrollY)
                assertEquals(ANCESTOR_SCROLL_Y, activity.richTextView.editorScrollView.scrollY)
            }
            withBlinkFrames(activity) { visible, hidden ->
                assertOutsideCaretEqual(visible, hidden)
            }
            instrumentation.runOnMainSyncWithResult {
                val connection =
                    requireNotNull(activity.editor.onCreateInputConnection(EditorInfo()))
                assertTrue(connection.commitText(NATIVE_EDIT, 1))
                assertTrue(requireNotNull(activity.adapter.documentHtml()).contains(NATIVE_EDIT))
                assertTrue(connection.deleteSurroundingText(NATIVE_EDIT.length, 0))
                assertEquals(authorized.third, activity.adapter.documentHtml())
                connection.closeConnection()
            }
        }

    @Test
    fun actualEmptyPlaceholderCompositionAndFocusTransitionsKeepCaretPaintOrder() =
        withEditor { activity ->
            instrumentation.runOnMainSyncWithResult {
                assertTrue(
                    activity.editor.applyUpdateJSON(
                        requireNotNull(activity.adapter.setContentJson(EMPTY_DOCUMENT))
                    )
                )
                activity.editor.placeholderText = PLACEHOLDER_TEXT
                assertTrue(activity.editor.shouldDisplayPlaceholderForTesting())
            }
            withBlinkFrames(activity) { visible, hidden ->
                assertOutsideCaretEqual(visible, hidden)
            }
            val beforeComposition = instrumentation.runOnMainSyncWithResult {
                val coreSelection = requireNotNull(activity.adapter.selectionJson())
                Triple(
                    activity.editor.text.toString(),
                    activity.editor.selectionStart to activity.editor.selectionEnd,
                    coreSelection
                )
            }
            val emptyHtml = instrumentation.runOnMainSyncWithResult {
                activity.adapter.documentHtml()
            }
            instrumentation.runOnMainSyncWithResult {
                activity.editor.beginExternalTextComposition(COMPOSITION_SESSION)
                activity.editor.updateExternalTextComposition(COMPOSITION_SESSION, COMPOSITION_TEXT)
                assertFalse(activity.editor.shouldDisplayPlaceholderForTesting())
                assertEquals(emptyHtml, activity.adapter.documentHtml())
            }
            withBlinkFrames(activity) { visible, hidden ->
                assertOutsideCaretEqual(visible, hidden)
            }
            instrumentation.runOnMainSyncWithResult {
                activity.editor.cancelExternalTextComposition(
                    COMPOSITION_SESSION,
                    COMPOSITION_CANCEL_CAUSE
                )
                assertTrue(activity.editor.shouldDisplayPlaceholderForTesting())
                assertEquals(emptyHtml, activity.adapter.documentHtml())
                assertEquals(
                    "Rollback must restore the committed precomposition raw text",
                    beforeComposition.first,
                    activity.editor.text.toString()
                )
                assertEquals(
                    "Rollback must restore the committed precomposition raw anchor",
                    beforeComposition.second.first,
                    activity.editor.selectionStart
                )
                assertEquals(
                    "Rollback must restore the committed precomposition raw head",
                    beforeComposition.second.second,
                    activity.editor.selectionEnd
                )
                assertEquals(
                    "Rollback must preserve every authoritative core selection field",
                    beforeComposition.third,
                    activity.adapter.selectionJson()
                )
            }
            val caret = capture(activity).use { Rect(it.caret) }
            instrumentation.runOnMainSyncWithResult { activity.editor.clearFocus() }
            capture(activity, caret).use {
                assertFalse(
                    "Focus loss must clear either retained or inline caret pixels",
                    it.hasCaret()
                )
            }
            instrumentation.runOnMainSyncWithResult { activity.editor.requestFocus() }
            withBlinkFrames(activity) { visible, hidden ->
                assertOutsideCaretEqual(visible, hidden)
            }
        }

    @Test
    fun legacyInputAlphaAnimationMatchesTheGenuineInlineCaret() = withEditor { activity ->
        assertLegacyAnimationMatchesInline(
            activity,
            start = { activity.editor.startAnimation(constantAlphaAnimation()) },
            clear = {
                instrumentation.runOnMainSyncWithResult { activity.editor.clearAnimation() }
            }
        )
    }

    @Test
    fun legacyInputTranslationMatchesTheGenuineInlineCaret() = withEditor { activity ->
        assertLegacyAnimationMatchesInline(
            activity,
            translationX = LEGACY_TRANSLATION_PX,
            start = {
                activity.editor.startAnimation(
                    TranslateAnimation(
                        LEGACY_TRANSLATION_PX.toFloat(),
                        LEGACY_TRANSLATION_PX.toFloat(),
                        0f,
                        0f
                    ).apply {
                        duration = LEGACY_ANIMATION_DURATION_MS
                        fillAfter = true
                    }
                )
            },
            clear = {
                instrumentation.runOnMainSyncWithResult { activity.editor.clearAnimation() }
            }
        )
    }

    @Test
    fun contentFrameLayoutAnimationMatchesTheGenuineInlineCaret() = withEditor { activity ->
        val content = activity.richTextView.editorContentFrame
        val original = instrumentation.runOnMainSyncWithResult {
            Pair(content.layoutAnimation, content.layoutAnimationListener)
        }
        val ended = CountDownLatch(1)
        val listener = object : Animation.AnimationListener {
            override fun onAnimationStart(animation: Animation) = Unit

            override fun onAnimationEnd(animation: Animation) {
                ended.countDown()
            }

            override fun onAnimationRepeat(animation: Animation) = Unit
        }
        assertLegacyAnimationMatchesInline(
            activity,
            start = {
                content.layoutAnimationListener = listener
                content.layoutAnimation =
                    LayoutAnimationController(constantAlphaAnimation(), 0f)
                content.startLayoutAnimation()
            },
            clear = {
                assertTrue(
                    "The owned layout animation must end before restoring its controller",
                    ended.await(
                        LEGACY_ANIMATION_DURATION_MS + FRAME_COMMIT_TIMEOUT_MS,
                        TimeUnit.MILLISECONDS
                    )
                )
                try {
                    awaitFrameCommit(activity)
                } finally {
                    instrumentation.runOnMainSyncWithResult {
                        for (index in 0 until content.childCount) {
                            content.getChildAt(index).clearAnimation()
                        }
                        content.layoutAnimationListener = original.second
                        content.layoutAnimation = original.first
                        content.invalidate()
                    }
                }
            }
        )
    }

    private fun constantAlphaAnimation(): AlphaAnimation =
        AlphaAnimation(LEGACY_ALPHA, LEGACY_ALPHA).apply {
            duration = LEGACY_ANIMATION_DURATION_MS
            fillAfter = true
        }

    private fun assertLegacyAnimationMatchesInline(
        activity: EditorSurfaceActivity,
        translationX: Int = 0,
        start: () -> Unit,
        clear: () -> Unit
    ) {
        val glyph = installGlyph(activity)
        val caret = capture(activity).use { Rect(it.caret).apply { offset(translationX, 0) } }
        assertTrue(
            "The animated fixture must contain a genuinely recorded glyph",
            glyph.hardwareDraws.get() > 0
        )
        val original = instrumentation.runOnMainSyncWithResult {
            Triple(
                activity.editor.foreground,
                activity.editor.text.toString(),
                activity.adapter.documentHtml()
            )
        }
        try {
            instrumentation.runOnMainSyncWithResult {
                activity.editor.clearFocus()
                start()
            }
            capture(activity, caret, awaitMainIdle = false).use { absent ->
                val absentState = animationPhaseState(activity, glyph)
                instrumentation.runOnMainSyncWithResult { activity.editor.requestFocus() }
                val visible: (Frame) -> Boolean = { it.hasCaretComparedTo(absent) }
                var phase = ANIMATION_PHASE_ACTUAL
                try {
                    withBlinkFrames(
                        activity,
                        caret,
                        visible,
                        awaitMainIdle = false
                    ) { actual, actualHidden ->
                        assertOutsideCaretEqual(actual, actualHidden)
                        phase = ANIMATION_PHASE_INLINE
                        instrumentation.runOnMainSyncWithResult {
                            activity.editor.foreground = ColorDrawable(Color.TRANSPARENT)
                        }
                        withBlinkFrames(
                            activity,
                            caret,
                            visible,
                            awaitMainIdle = false
                        ) { inline, inlineHidden ->
                            assertOutsideCaretEqual(inline, inlineHidden)
                            assertFramePixelsEqual(actualHidden, inlineHidden)
                            assertFramePixelsEqual(actual, inline)
                        }
                    }
                } catch (failure: AssertionError) {
                    try {
                        val prefix = "$ANIMATION_PHASE_FAILURE_PREFIX-${SystemClock.uptimeMillis()}"
                        val absentFile = instrumentation.saveDeviceScreenshot(
                            "$prefix-absent.png",
                            absent.pixels
                        )
                        capture(activity, caret, awaitMainIdle = false).use { current ->
                            val currentFile = instrumentation.saveDeviceScreenshot(
                                "$prefix-current.png",
                                current.pixels
                            )
                            val diagnosticsFile = File(absentFile.parentFile, "$prefix.json")
                            diagnosticsFile.writeText(
                                JSONObject()
                                    .put("phase", phase)
                                    .put("assertion", failure.message)
                                    .put("absent", absent.diagnostics)
                                    .put("absentState", absentState)
                                    .put("current", current.diagnostics)
                                    .put("currentState", animationPhaseState(activity, glyph))
                                    .put("caretComparison", current.caretComparison(absent))
                                    .put("absentPng", absentFile.absolutePath)
                                    .put("currentPng", currentFile.absolutePath)
                                    .toString(2)
                            )
                            failure.addSuppressed(
                                AssertionError(
                                    "Animation phase diagnostics=${diagnosticsFile.absolutePath}"
                                )
                            )
                        }
                    } catch (diagnosticFailure: AssertionError) {
                        failure.addSuppressed(diagnosticFailure)
                    } catch (diagnosticFailure: IOException) {
                        failure.addSuppressed(diagnosticFailure)
                    }
                    throw failure
                }
            }
        } finally {
            instrumentation.runOnMainSyncWithResult {
                activity.editor.foreground = original.first
            }
            clear()
        }
        instrumentation.runOnMainSyncWithResult {
            assertEquals(
                "Animation must preserve the live text",
                original.second,
                activity.editor.text.toString()
            )
            assertEquals(
                "Animation must preserve the genuine core document",
                original.third,
                activity.adapter.documentHtml()
            )
        }
    }

    private fun animationPhaseState(
        activity: EditorSurfaceActivity,
        glyph: RecordingGlyph
    ): JSONObject = instrumentation.runOnMainSyncWithResult {
        val input = activity.editor
        val animation = input.animation
        val layoutAnimation = (input.parent as? ViewGroup)?.layoutAnimation
        JSONObject()
            .put("capturedUptimeMs", SystemClock.uptimeMillis())
            .put("nativeCaret", input.nativeCursorDrawRect()?.toString() ?: JSONObject.NULL)
            .put("selection", JSONArray(listOf(input.selectionStart, input.selectionEnd)))
            .put("inputAlpha", input.alpha)
            .put("inputMatrix", input.matrix.toString())
            .put("inputAnimation", animation?.javaClass?.name ?: JSONObject.NULL)
            .put("inputAnimationStarted", animation?.hasStarted() ?: JSONObject.NULL)
            .put("inputAnimationEnded", animation?.hasEnded() ?: JSONObject.NULL)
            .put("inputAnimationStartTime", animation?.startTime ?: JSONObject.NULL)
            .put("inputAnimationDuration", animation?.duration ?: JSONObject.NULL)
            .put("parentLayoutAnimation", layoutAnimation?.javaClass?.name ?: JSONObject.NULL)
            .put("foreground", input.foreground?.javaClass?.name ?: JSONObject.NULL)
            .put("glyphHardwareDraws", glyph.hardwareDraws.get())
            .put("glyphSoftwareDraws", glyph.softwareDraws.get())
    }

    @Test
    fun collapsedInputConnectionMovesReuseBodyWhileRangeTransitionsKeepSelectionPaint() =
        withEditor { activity ->
            val glyph = installGlyph(activity)
            capture(activity).close()
            var referencePixels: Bitmap? = null
            var ownedConnection: android.view.inputmethod.InputConnection? = null
            val selectionEvents = mutableListOf<Pair<Int, Int>>()
            val updates = mutableListOf<String>()
            val originalListener = instrumentation.runOnMainSyncWithResult {
                activity.editor.editorListener
            }
            try {
                withBlinkFrames(activity) { visible, hidden ->
                    assertOutsideCaretEqual(visible, hidden)
                    referencePixels =
                        requireNotNull(hidden.pixels.copy(Bitmap.Config.ARGB_8888, false))
                }
                val connection = instrumentation.runOnMainSyncWithResult {
                    activity.editor.editorListener =
                        object : com.apollohg.editor.EditorEditText.EditorListener {
                            override fun onSelectionChanged(anchor: Int, head: Int) {
                                selectionEvents += anchor to head
                                originalListener?.onSelectionChanged(anchor, head)
                            }
                            override fun onEditorUpdate(updateJSON: String) {
                                updates += updateJSON
                                originalListener?.onEditorUpdate(updateJSON)
                            }
                            override fun onExternalTextCompositionEnded(resultJson: String) {
                                originalListener?.onExternalTextCompositionEnded(resultJson)
                            }
                        }
                    requireNotNull(activity.editor.onCreateInputConnection(EditorInfo())).also {
                        ownedConnection = it
                    }
                }
                val original = instrumentation.runOnMainSyncWithResult {
                    Triple(
                        activity.editor.text.toString(),
                        activity.adapter.baseDocumentRevision,
                        activity.adapter.documentHtml()
                    )
                }
                val beforeMoves = glyph.hardwareDraws.get()
                assertTrue(
                    "The genuine document glyph must already be hardware recorded",
                    beforeMoves > 0
                )
                val targets = listOf(
                    CARET_OFFSET + COLLAPSED_MOVE_DISTANCE,
                    CARET_OFFSET + SELECTION_RANGE_DISTANCE,
                    CARET_OFFSET
                )
                for (target in targets) {
                    instrumentation.runOnMainSyncWithResult {
                        val input = activity.editor
                        assertTrue(
                            "Each real IC operation must move the collapsed caret",
                            input.selectionEnd != target
                        )
                        val eventCount = selectionEvents.size
                        val stateRevision = activity.adapter.stateRevision
                        assertTrue(connection.setSelection(target, target))
                        assertEquals(target, input.selectionStart)
                        assertEquals(target, input.selectionEnd)
                        assertEquals(original.first, input.text.toString())
                        assertEquals(original.second, activity.adapter.baseDocumentRevision)
                        assertTrue(
                            "Native selection must advance the real engine state",
                            activity.adapter.stateRevision > stateRevision
                        )
                        val scalar = PositionBridge.utf16ToScalar(target, input.text.toString())
                        assertEquals(
                            listOf(scalar, scalar),
                            activity.adapter.cachedAuthoritativeScalarSelection?.toList()
                        )
                        assertEquals(
                            "Each accepted IC move must deliver one actual selection event",
                            eventCount + 1,
                            selectionEvents.size
                        )
                        val snapshot =
                            JSONObject(
                                requireNotNull(
                                    activity.adapter.atomicRenderJson(original.second.toString())
                                )
                            )
                        assertEquals(PUBLIC_SNAPSHOT_FIELD_COUNT, snapshot.length())
                        assertFalse(
                            "The public canonical snapshot must remain epoch-free",
                            snapshot.has("positionEpoch")
                        )
                        assertEquals(
                            original.second.toString(),
                            snapshot.getString("documentVersion")
                        )
                        assertEquals(
                            activity.adapter.stateRevision.toString(),
                            snapshot.getString("stateRevision")
                        )
                        val selection = snapshot.getJSONObject("selection")
                        assertEquals(
                            selection.getInt("anchor") to selection.getInt("head"),
                            selectionEvents.last()
                        )
                        assertEquals(
                            "A selection-only operation must publish no document update",
                            emptyList<String>(),
                            updates
                        )
                        assertTrue(input.hasFocus() && input.hasWindowFocus())
                    }
                    withBlinkFrames(activity) { visible, hidden ->
                        assertOutsideCaretEqual(visible, hidden)
                        assertTrue(
                            "Each new native caret must retain exact document pixels with no old caret left behind",
                            hidden.pixels.sameAs(requireNotNull(referencePixels))
                        )
                        assertTrue(
                            "The actual document glyph must remain visible",
                            visible.hasColor(GLYPH_COLOR)
                        )
                    }
                }
                val afterMoves = glyph.hardwareDraws.get()
                instrumentation.runOnMainSyncWithResult {
                    assertTrue(
                        connection.setSelection(
                            CARET_OFFSET,
                            CARET_OFFSET + SELECTION_RANGE_DISTANCE
                        )
                    )
                    assertEquals(CARET_OFFSET, activity.editor.selectionStart)
                    assertEquals(
                        CARET_OFFSET + SELECTION_RANGE_DISTANCE,
                        activity.editor.selectionEnd
                    )
                }
                capture(activity).use { frame ->
                    assertFalse(
                        "A real expanded selection must hide the collapsed native caret",
                        frame.hasCaret()
                    )
                    assertFalse(
                        "The actual selection highlight and handles must change the document pixels",
                        frame.pixels.sameAs(requireNotNull(referencePixels))
                    )
                }
                val afterRange = glyph.hardwareDraws.get()
                assertTrue(
                    "Expanding the selection must repaint the body highlight and handles",
                    afterRange > afterMoves
                )
                instrumentation.runOnMainSyncWithResult {
                    assertTrue(connection.setSelection(CARET_OFFSET, CARET_OFFSET))
                    assertEquals(original.first, activity.editor.text.toString())
                    assertEquals(original.second, activity.adapter.baseDocumentRevision)
                    assertEquals(original.third, activity.adapter.documentHtml())
                }
                withBlinkFrames(activity) { visible, hidden ->
                    assertOutsideCaretEqual(visible, hidden)
                    assertTrue(
                        "Collapsing a range must remove the actual highlight and handles",
                        hidden.pixels.sameAs(requireNotNull(referencePixels))
                    )
                }
                assertTrue(
                    "Range-to-caret transition must repaint the body to clear old selection pixels",
                    glyph.hardwareDraws.get() > afterRange
                )
                assertEquals(
                    "Only collapsed IC movements must reuse the existing body recording; before=$beforeMoves after=$afterMoves events=$selectionEvents",
                    beforeMoves,
                    afterMoves
                )
            } finally {
                instrumentation.runOnMainSyncWithResult {
                    activity.editor.editorListener = originalListener
                    ownedConnection?.closeConnection()
                }
                referencePixels?.recycle()
            }
        }

    @Test
    fun japaneseCompositionBuildsOnlyTheFinalStyledLayoutAndPublishesTheNativeCommit() =
        withEditor { activity ->
            instrumentation.runOnMainSyncWithResult {
                val update =
                    requireNotNull(activity.adapter.setContentHtml(COMPOSING_DOCUMENT_HTML))
                assertTrue(activity.editor.applyUpdateJSON(update))
                activity.editor.setSelection(CARET_OFFSET)
            }
            capture(activity).close()
            val events = mutableListOf<Pair<Int, Int>>()
            val updates = mutableListOf<String>()
            val originalListener = instrumentation.runOnMainSyncWithResult {
                activity.editor.editorListener
            }
            var ownedConnection: EditorInputConnection? = null
            val builds = JSONArray()
            try {
                val connection = instrumentation.runOnMainSyncWithResult {
                    activity.editor.editorListener =
                        object : com.apollohg.editor.EditorEditText.EditorListener {
                            override fun onSelectionChanged(anchor: Int, head: Int) {
                                events += anchor to head
                                originalListener?.onSelectionChanged(anchor, head)
                            }
                            override fun onEditorUpdate(updateJSON: String) {
                                updates += updateJSON
                                originalListener?.onEditorUpdate(updateJSON)
                            }
                            override fun onExternalTextCompositionEnded(resultJson: String) {
                                originalListener?.onExternalTextCompositionEnded(resultJson)
                            }
                        }
                    (
                        requireNotNull(activity.editor.onCreateInputConnection(EditorInfo())) as
                            EditorInputConnection
                        ).also { ownedConnection = it }
                }
                fun renderedStyle(offset: Int): Triple<Float, Int, Boolean> {
                    val input = activity.editor
                    val paint = TextPaint(input.paint)
                    input.text.getSpans(offset, offset + 1, CharacterStyle::class.java)
                        .forEach { it.updateDrawState(paint) }
                    return Triple(paint.textSize, paint.color, paint.typeface?.isBold == true)
                }
                val original = instrumentation.runOnMainSyncWithResult {
                    Triple(
                        activity.editor.text.toString(),
                        requireNotNull(activity.adapter.documentHtml()),
                        activity.adapter.baseDocumentRevision to activity.adapter.stateRevision
                    )
                }
                val expectedStyle = instrumentation.runOnMainSyncWithResult {
                    assertTrue(
                        "The genuine core fixture must export its styled text",
                        original.second.contains(COMPOSING_BODY_TEXT)
                    )
                    assertTrue(
                        "The mounted fixture must begin with a collapsed focused caret",
                        activity.editor.hasFocus() && activity.editor.hasWindowFocus() &&
                            activity.editor.selectionStart == CARET_OFFSET &&
                            activity.editor.selectionEnd == CARET_OFFSET
                    )
                    renderedStyle(CARET_OFFSET).also {
                        assertTrue("The source text must actually render bold", it.third)
                    }
                }
                for (reading in JAPANESE_COMPOSING_TEXTS) {
                    val expectedText = original.first.replaceRange(
                        CARET_OFFSET,
                        CARET_OFFSET,
                        reading
                    )
                    val expectedEnd = CARET_OFFSET + reading.length
                    val sample = instrumentation.runOnMainSyncWithResult {
                        val input = activity.editor
                        val before = input.documentLayoutBuildCount
                        assertTrue(connection.setComposingText(reading, CURSOR_AFTER_INSERTION))
                        val afterReturn = input.documentLayoutBuildCount
                        assertEquals(
                            "The actual provisional reading must be visible",
                            expectedText,
                            input.text.toString()
                        )
                        assertEquals(expectedEnd, input.selectionStart)
                        assertEquals(expectedEnd, input.selectionEnd)
                        assertEquals(
                            CARET_OFFSET,
                            BaseInputConnection.getComposingSpanStart(input.text)
                        )
                        assertEquals(
                            expectedEnd,
                            BaseInputConnection.getComposingSpanEnd(input.text)
                        )
                        val styles = input.text.getSpans(
                            CARET_OFFSET,
                            expectedEnd,
                            TransientComposingTextStyleSpan::class.java
                        )
                        assertTrue(
                            "Composition must use real transient style spans",
                            styles.isNotEmpty()
                        )
                        for (span in styles) {
                            assertEquals(CARET_OFFSET, input.text.getSpanStart(span))
                            assertEquals(expectedEnd, input.text.getSpanEnd(span))
                        }
                        assertEquals(
                            "The provisional glyphs must retain the source's size, color and bold style",
                            expectedStyle,
                            renderedStyle(CARET_OFFSET)
                        )
                        assertEquals(
                            "Composition must preserve the real canonical document",
                            original.second,
                            activity.adapter.documentHtml()
                        )
                        assertEquals(original.third.first, activity.adapter.baseDocumentRevision)
                        assertEquals(original.third.second, activity.adapter.stateRevision)
                        assertEquals(
                            "Transient composition must publish no public selection mutation",
                            emptyList<Pair<Int, Int>>(),
                            events
                        )
                        assertEquals(
                            "Transient composition must publish no document update",
                            emptyList<String>(),
                            updates
                        )
                        val imeSelection =
                            listOf(expectedEnd, expectedEnd, CARET_OFFSET, expectedEnd)
                        assertEquals(
                            "The real IC must publish the provisional IME selection",
                            imeSelection,
                            connection.lastPublishedSelection
                        )
                        val published = requireNotNull(connection.lastPublishedRawInputState)
                        assertEquals(expectedText, published.text)
                        assertEquals(imeSelection, published.selection)
                        val extracted =
                            requireNotNull(connection.getExtractedText(ExtractedTextRequest(), 0))
                        assertEquals(
                            "The public IC must return the exact visible provisional text",
                            expectedText,
                            extracted.text.toString()
                        )
                        assertEquals(expectedEnd, extracted.selectionStart)
                        assertEquals(expectedEnd, extracted.selectionEnd)
                        JSONObject().put("reading", reading).put("before", before)
                            .put("immediatelyAfterReturn", afterReturn)
                    }
                    withBlinkFrames(activity) { visible, hidden ->
                        assertOutsideCaretEqual(visible, hidden)
                        assertEquals(
                            "The committed hardware frame must contain the current reading",
                            expectedText,
                            visible.diagnostics.getString("layoutText")
                        )
                    }
                    instrumentation.runOnMainSyncWithResult {
                        val input = activity.editor
                        assertEquals(expectedText, input.text.toString())
                        assertEquals(expectedEnd, input.selectionEnd)
                        assertEquals(expectedStyle, renderedStyle(CARET_OFFSET))
                        val caret = requireNotNull(input.nativeCursorDrawRect())
                        assertTrue(
                            "The settled real native caret must have finite positive bounds",
                            listOf(caret.left, caret.top, caret.right, caret.bottom).all {
                                it.isFinite()
                            } &&
                                caret.width() > 0f && caret.height() > 0f
                        )
                        sample.put("afterSettledHardwareFrame", input.documentLayoutBuildCount)
                        builds.put(sample)
                    }
                }
                instrumentation.runOnMainSyncWithResult {
                    assertTrue(
                        connection.commitText(JAPANESE_COMMITTED_TEXT, CURSOR_AFTER_INSERTION)
                    )
                }
                capture(activity).close()
                instrumentation.runOnMainSyncWithResult {
                    val input = activity.editor
                    val expectedText = original.first.replaceRange(
                        CARET_OFFSET,
                        CARET_OFFSET,
                        JAPANESE_COMMITTED_TEXT
                    )
                    val expectedBody = COMPOSING_BODY_TEXT.replaceRange(
                        CARET_OFFSET,
                        CARET_OFFSET,
                        JAPANESE_COMMITTED_TEXT
                    )
                    assertEquals(expectedText, input.text.toString())
                    assertEquals(
                        "The genuine core must commit exactly the styled Japanese insertion",
                        original.second.replaceFirst(COMPOSING_BODY_TEXT, expectedBody),
                        activity.adapter.documentHtml()
                    )
                    assertEquals(
                        CARET_OFFSET + JAPANESE_COMMITTED_TEXT.length,
                        input.selectionStart
                    )
                    assertEquals(input.selectionStart, input.selectionEnd)
                    assertEquals(
                        NO_COMPOSING_SPAN,
                        BaseInputConnection.getComposingSpanStart(input.text)
                    )
                    assertEquals(
                        NO_COMPOSING_SPAN,
                        BaseInputConnection.getComposingSpanEnd(input.text)
                    )
                    assertEquals(
                        "Committed text must contain no leftover transient composing styles",
                        0,
                        input.text.getSpans(
                            0,
                            input.text.length,
                            TransientComposingTextStyleSpan::class.java
                        ).size
                    )
                    assertEquals(
                        "The committed text must retain its actual rendered style",
                        expectedStyle,
                        renderedStyle(CARET_OFFSET)
                    )
                    assertTrue(activity.adapter.baseDocumentRevision > original.third.first)
                    assertTrue(activity.adapter.stateRevision > original.third.second)
                    val scalar = PositionBridge.utf16ToScalar(
                        input.selectionEnd,
                        input.text.toString()
                    )
                    assertEquals(
                        listOf(scalar, scalar),
                        activity.adapter.cachedAuthoritativeScalarSelection?.toList()
                    )
                    assertEquals(
                        "The real commit must deliver one public document update",
                        1,
                        updates.size
                    )
                    val snapshot =
                        JSONObject(
                            requireNotNull(
                                activity.adapter.atomicRenderJson(
                                    activity.adapter.baseDocumentRevision.toString()
                                )
                            )
                        )
                    assertEquals(PUBLIC_SNAPSHOT_FIELD_COUNT, snapshot.length())
                    assertFalse(
                        "The public canonical commit must remain epoch-free",
                        snapshot.has("positionEpoch")
                    )
                    assertEquals(
                        activity.adapter.baseDocumentRevision.toString(),
                        snapshot.getString("documentVersion")
                    )
                    assertEquals(
                        activity.adapter.stateRevision.toString(),
                        snapshot.getString("stateRevision")
                    )
                    val selection = snapshot.getJSONObject("selection")
                    assertEquals(scalar, selection.getInt("anchorScalar"))
                    assertEquals(scalar, selection.getInt("headScalar"))
                }
                val diagnostics =
                    File(
                        requireNotNull(instrumentation.targetContext.getExternalFilesDir(null)),
                        "$IME_LAYOUT_FAILURE_PREFIX-${SystemClock.uptimeMillis()}.json"
                    )
                diagnostics.writeText(JSONObject().put("compositionBuilds", builds).toString(2))
                for (index in 0 until builds.length()) {
                    val sample = builds.getJSONObject(index)
                    val before = sample.getInt("before")
                    val detail = "diagnostics=${diagnostics.absolutePath} composition=$sample"
                    assertEquals(
                        "Composition must defer layout until all transient styles are applied; $detail",
                        before,
                        sample.getInt("immediatelyAfterReturn")
                    )
                    assertEquals(
                        "The settled composition must build only its final styled layout; $detail",
                        before + FINAL_COMPOSITION_LAYOUT_BUILDS,
                        sample.getInt("afterSettledHardwareFrame")
                    )
                }
            } finally {
                instrumentation.runOnMainSyncWithResult {
                    activity.editor.editorListener = originalListener
                    ownedConnection?.closeConnection()
                }
            }
        }

    private fun withEditor(block: (EditorSurfaceActivity) -> Unit) {
        ActivityScenario.launch<EditorSurfaceActivity>(
            Intent(
                instrumentation.targetContext,
                EditorSurfaceActivity::class.java
            ).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        ).use { scenario ->
            lateinit var activity: EditorSurfaceActivity
            scenario.onActivity {
                activity = it
                it.editor.showSoftInputOnFocus = false
                it.editor.isVerticalScrollBarEnabled = false
                it.richTextView.editorScrollView.isVerticalScrollBarEnabled = false
                it.editor.requestFocus()
                it.editor.setSelection(CARET_OFFSET)
                assertTrue(it.window.decorView.isHardwareAccelerated)
            }
            instrumentation.waitForIdleSync()
            val point = instrumentation.runOnMainSyncWithResult {
                val input = activity.editor
                val location = IntArray(2).also(input::getLocationOnScreen)
                val caret = requireNotNull(input.nativeCursorDrawRect())
                location[0] + input.compoundPaddingLeft + caret.centerX() to
                    location[1] + input.compoundPaddingTop + caret.centerY()
            }
            val down = SystemClock.uptimeMillis()
            for (action in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) {
                val event = MotionEvent.obtain(
                    down,
                    SystemClock.uptimeMillis(),
                    action,
                    point.first,
                    point.second,
                    0
                )
                try {
                    assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true))
                } finally {
                    event.recycle()
                }
            }
            instrumentation.runOnMainSyncWithResult {
                activity.editor.setSelection(CARET_OFFSET + 1)
                activity.editor.setSelection(CARET_OFFSET)
                assertTrue(activity.editor.hasFocus() && activity.editor.hasWindowFocus())
                assertTrue(activity.editor.isInTouchMode)
            }
            instrumentation.waitForIdleSync()
            block(activity)
        }
    }

    private fun installGlyph(activity: EditorSurfaceActivity): RecordingGlyph =
        instrumentation.runOnMainSyncWithResult {
            RecordingGlyph().also {
                activity.editor.text.setSpan(it, 0, 1, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                activity.editor.setSelection(CARET_OFFSET + 1)
                activity.editor.setSelection(CARET_OFFSET)
            }
        }

    private fun withBlinkFrames(
        activity: EditorSurfaceActivity,
        caretOverride: Rect? = null,
        isVisible: (Frame) -> Boolean = { it.hasCaret() },
        awaitMainIdle: Boolean = true,
        block: (Frame, Frame) -> Unit
    ) {
        var visible: Frame? = null
        var hidden: Frame? = null
        val deadline = SystemClock.uptimeMillis() + PHASE_TIMEOUT_MS
        try {
            while ((visible == null || hidden == null) && SystemClock.uptimeMillis() < deadline) {
                val frame = capture(activity, caretOverride, awaitMainIdle)
                val hasCaret = isVisible(frame)
                if (hasCaret && visible == null) {
                    visible = frame
                } else if (!hasCaret && hidden == null) {
                    hidden = frame
                } else {
                    frame.close()
                }
                if (visible == null || hidden == null) SystemClock.sleep(POLL_INTERVAL_MS)
            }
            assertNotNull("Natural blink must produce a genuine visible caret screenshot", visible)
            assertNotNull("Natural blink must produce a genuine hidden caret screenshot", hidden)
            block(requireNotNull(visible), requireNotNull(hidden))
        } finally {
            visible?.close()
            hidden?.close()
        }
    }

    private fun awaitFrameCommit(activity: EditorSurfaceActivity): Long {
        val committed = CountDownLatch(1)
        var committedAt = 0L
        val callback = Runnable {
            committedAt = SystemClock.uptimeMillis()
            committed.countDown()
        }
        val observer = instrumentation.runOnMainSyncWithResult {
            val viewport = activity.richTextView.editorViewport
            assertTrue(
                "Frame commit capture needs the genuine attached hardware viewport",
                viewport.isAttachedToWindow && viewport.isHardwareAccelerated
            )
            viewport.viewTreeObserver.also {
                it.registerFrameCommitCallback(callback)
                viewport.invalidate()
            }
        }
        try {
            assertTrue(
                "The actual hardware frame must commit before screenshot capture",
                committed.await(FRAME_COMMIT_TIMEOUT_MS, TimeUnit.MILLISECONDS)
            )
        } finally {
            instrumentation.runOnMainSyncWithResult {
                if (observer.isAlive) observer.unregisterFrameCommitCallback(callback)
            }
        }
        return committedAt
    }

    private fun capture(
        activity: EditorSurfaceActivity,
        caretOverride: Rect? = null,
        awaitMainIdle: Boolean = true
    ): Frame {
        if (awaitMainIdle) instrumentation.waitForIdleSync()
        val committedAt = awaitFrameCommit(activity)
        val geometry = instrumentation.runOnMainSyncWithResult {
            val viewport = activity.richTextView.editorViewport
            val input = activity.editor
            assertFalse(
                "The document pixel fixture must keep auxiliary input scrollbars disabled",
                input.isVerticalScrollBarEnabled
            )
            assertFalse(
                "The document pixel fixture must keep auxiliary ancestor scrollbars disabled",
                activity.richTextView.editorScrollView.isVerticalScrollBarEnabled
            )
            val location = IntArray(2).also(viewport::getLocationOnScreen)
            val inputLocation = IntArray(2).also(input::getLocationOnScreen)
            val caret = caretOverride ?: RectF(requireNotNull(input.nativeCursorDrawRect())).apply {
                offset(
                    (
                        inputLocation[0] - location[0] +
                            input.compoundPaddingLeft - input.scrollX
                        ).toFloat(),
                    (
                        inputLocation[1] - location[1] +
                            input.compoundPaddingTop - input.scrollY
                        ).toFloat()
                )
            }.let {
                Rect(
                    floor(it.left).toInt(),
                    floor(it.top).toInt(),
                    ceil(it.right).toInt(),
                    ceil(it.bottom).toInt()
                )
            }
            val viewportBounds = Rect(
                location[0],
                location[1],
                location[0] + viewport.width,
                location[1] + viewport.height
            )
            val diagnostics = JSONObject()
                .put("geometryCapturedUptimeMs", SystemClock.uptimeMillis())
                .put("frameCommittedUptimeMs", committedAt)
                .put("viewport", viewportBounds.toShortString())
                .put("caret", caret.toShortString())
                .put("caretColor", Integer.toHexString(input.caretColor))
                .put("inputLocation", JSONArray(inputLocation.toList()))
                .put("inputSize", JSONArray(listOf(input.width, input.height)))
                .put("inputScroll", JSONArray(listOf(input.scrollX, input.scrollY)))
                .put("ancestorScrollY", activity.richTextView.editorScrollView.scrollY)
                .put("selection", JSONArray(listOf(input.selectionStart, input.selectionEnd)))
                .put("text", input.text.toString())
                .put("layoutText", input.layout.text.toString())
                .put("layoutHeight", input.layout.height)
                .put("coreDocumentIsEmpty", input.coreReportedDocumentIsEmpty ?: JSONObject.NULL)
                .put("placeholder", input.placeholderText)
                .put("placeholderDisplayed", input.shouldDisplayPlaceholderForTesting())
                .put("hasFocus", input.hasFocus())
                .put("hasWindowFocus", input.hasWindowFocus())
                .put("hardwareAccelerated", input.isHardwareAccelerated)
                .put("documentRevision", activity.adapter.baseDocumentRevision.toString())
            Pair(Triple(viewportBounds, caret, input.caretColor), diagnostics)
        }
        val screenshot = requireNotNull(instrumentation.uiAutomation.takeScreenshot())
        try {
            if (!awaitMainIdle) {
                instrumentation.runOnMainSyncWithResult {
                    val animation = activity.editor.animation
                    val started = animation?.hasStarted() == true
                    val ended = animation?.hasEnded() != false
                    geometry.second
                        .put("animationAssertionCapturedUptimeMs", SystemClock.uptimeMillis())
                        .put("activeAnimationStarted", started)
                        .put("activeAnimationEnded", ended)
                    assertTrue(
                        "The committed animation frame must show the genuine animation started " +
                            "and still active: ${geometry.second}",
                        started && !ended
                    )
                }
            }
            val viewport = geometry.first.first
            val cropped = Bitmap.createBitmap(
                screenshot,
                viewport.left,
                viewport.top,
                viewport.width(),
                viewport.height()
            )
            val independent = requireNotNull(cropped.copy(Bitmap.Config.ARGB_8888, false))
            if (cropped !== screenshot) cropped.recycle()
            return Frame(
                independent,
                geometry.first.second,
                geometry.first.third,
                geometry.second
                    .put("screenshotCapturedUptimeMs", SystemClock.uptimeMillis())
            )
        } finally {
            screenshot.recycle()
        }
    }

    private fun assertOutsideCaretEqual(visible: Frame, hidden: Frame) {
        assertEquals(
            "Caret must stay at the same native geometry between blink phases",
            visible.caret,
            hidden.caret
        )
        assertEquals(visible.pixels.width, hidden.pixels.width)
        assertEquals(visible.pixels.height, hidden.pixels.height)
        val excluded = Rect(visible.caret).apply { inset(-CARET_PIXEL_MARGIN, -CARET_PIXEL_MARGIN) }
        assertFramePixelsEqual(visible, hidden, excluded)
    }

    private fun assertFramePixelsEqual(visible: Frame, hidden: Frame, excluded: Rect = Rect()) {
        assertEquals(visible.pixels.width, hidden.pixels.width)
        assertEquals(visible.pixels.height, hidden.pixels.height)
        val width = visible.pixels.width
        val height = visible.pixels.height
        val before = IntArray(width * height)
        val after = IntArray(before.size)
        visible.pixels.getPixels(before, 0, width, 0, 0, width, height)
        hidden.pixels.getPixels(after, 0, width, 0, 0, width, height)
        for (index in before.indices) {
            val x = index % width
            val y = index / width
            if (!excluded.contains(x, y) && before[index] != after[index]) {
                val prefix = "$PIXEL_FAILURE_PREFIX-${SystemClock.uptimeMillis()}"
                val visibleFile = instrumentation.saveDeviceScreenshot(
                    "$prefix-visible.png",
                    visible.pixels
                )
                val hiddenFile = instrumentation.saveDeviceScreenshot(
                    "$prefix-hidden.png",
                    hidden.pixels
                )
                val diagnosticsFile = File(visibleFile.parentFile, "$prefix.json")
                diagnosticsFile.writeText(
                    JSONObject()
                        .put("visible", visible.diagnostics)
                        .put("hidden", hidden.diagnostics)
                        .put("visiblePng", visibleFile.absolutePath)
                        .put("hiddenPng", hiddenFile.absolutePath)
                        .put(
                            "firstMismatch",
                            JSONObject().put("x", x).put("y", y)
                                .put("visibleArgb", Integer.toHexString(before[index]))
                                .put("hiddenArgb", Integer.toHexString(after[index]))
                        )
                        .toString(2)
                )
                assertEquals(
                    "Blink changed document pixel ($x,$y) outside the native caret $excluded; " +
                        "diagnostics=${diagnosticsFile.absolutePath} " +
                        "visible=${visibleFile.absolutePath} " +
                        "hidden=${hiddenFile.absolutePath}",
                    before[index],
                    after[index]
                )
            }
        }
    }

    private class Frame(
        val pixels: Bitmap,
        val caret: Rect,
        private val caretColor: Int,
        val diagnostics: JSONObject
    ) : Closeable {
        fun hasCaret(): Boolean = caretMatches { x, y ->
            closeColor(pixels.getPixel(x, y), caretColor)
        }

        fun hasCaretComparedTo(absent: Frame): Boolean = caretMatches { x, y ->
            pixels.getPixel(x, y) != absent.pixels.getPixel(x, y)
        }

        fun caretComparison(absent: Frame): JSONObject {
            var sampled = 0
            var different = 0
            var firstDifference: JSONObject? = null
            val matches = caretMatches { x, y ->
                sampled++
                val actual = pixels.getPixel(x, y)
                val expected = absent.pixels.getPixel(x, y)
                if (actual != expected) {
                    different++
                    if (firstDifference == null) {
                        firstDifference = JSONObject().put("x", x).put("y", y)
                            .put("absentArgb", Integer.toHexString(expected))
                            .put("currentArgb", Integer.toHexString(actual))
                    }
                }
                actual != expected
            }
            return JSONObject()
                .put("sampledPixels", sampled)
                .put("differentPixels", different)
                .put("samePixels", sampled - different)
                .put("differentFraction", if (sampled == 0) 0f else different.toFloat() / sampled)
                .put("matchedExistingVisibilityPredicate", matches)
                .put("firstDifference", firstDifference ?: JSONObject.NULL)
        }

        private fun caretMatches(matches: (Int, Int) -> Boolean): Boolean {
            var matching = 0
            var sampled = 0
            for (y in caret.top.coerceAtLeast(0) until caret.bottom.coerceAtMost(pixels.height)) {
                for (x in caret.left.coerceAtLeast(
                    0
                ) until caret.right.coerceAtMost(pixels.width)) {
                    sampled++
                    if (matches(x, y)) matching++
                }
            }
            return sampled > 0 && matching.toFloat() / sampled >= CARET_MATCH_FRACTION
        }

        fun hasColor(color: Int): Boolean = (0 until pixels.height).any { y ->
            (0 until pixels.width).any { x -> closeColor(pixels.getPixel(x, y), color) }
        }

        override fun close() = pixels.recycle()
    }

    private class RecordingGlyph : ReplacementSpan() {
        val hardwareDraws = AtomicInteger()
        val softwareDraws = AtomicInteger()
        var color = GLYPH_COLOR

        override fun getSize(
            paint: Paint,
            text: CharSequence,
            start: Int,
            end: Int,
            fm: Paint.FontMetricsInt?
        ): Int {
            if (fm != null) paint.getFontMetricsInt(fm)
            return ceil(paint.measureText(text, start, end).toDouble()).toInt().coerceAtLeast(1)
        }

        override fun draw(
            canvas: Canvas,
            text: CharSequence,
            start: Int,
            end: Int,
            x: Float,
            top: Int,
            y: Int,
            bottom: Int,
            paint: Paint
        ) {
            if (canvas.isHardwareAccelerated) {
                hardwareDraws.incrementAndGet()
            } else {
                softwareDraws.incrementAndGet()
            }
            val glyphPaint = Paint(paint).apply { color = this@RecordingGlyph.color }
            canvas.drawRect(
                x,
                y + paint.fontMetrics.ascent,
                x + getSize(paint, text, start, end, null),
                y + paint.fontMetrics.descent,
                glyphPaint
            )
        }
    }

    companion object {
        private const val GLYPH_RECORDING_FIXTURE = "very-large"
        private const val GLYPH_FALLBACK_SCALE = .98f
        private const val HIDDEN_VIEWPORT_DISTANCE = 2
        private const val VIEWPORT_SETTLE_WINDOW_MS = 250L
        private const val IDLE_FRAME_BUDGET = 2
        private const val CARET_OFFSET = 5
        private const val COLLAPSED_MOVE_DISTANCE = 1
        private const val SELECTION_RANGE_DISTANCE = 2
        private const val PUBLIC_SNAPSHOT_FIELD_COUNT = 9
        private const val COMPOSING_BODY_TEXT = "Alpha beta gamma."
        private const val COMPOSING_DOCUMENT_HTML = "<p><strong>$COMPOSING_BODY_TEXT</strong></p>"
        private val JAPANESE_COMPOSING_TEXTS = listOf("に", "にほん")
        private const val JAPANESE_COMMITTED_TEXT = "日本語"
        private const val CURSOR_AFTER_INSERTION = 1
        private const val NO_COMPOSING_SPAN = -1
        private const val FINAL_COMPOSITION_LAYOUT_BUILDS = 1
        private const val IME_LAYOUT_FAILURE_PREFIX = "caret-ime-layout"
        private const val FIRST_PARAGRAPH_END = 25
        private const val PHASE_TIMEOUT_MS = 4000L
        private const val LEGACY_ANIMATION_DURATION_MS = 12_000L
        private const val LEGACY_ALPHA = .25f
        private const val LEGACY_TRANSLATION_PX = 24
        private const val POLL_INTERVAL_MS = 40L
        private const val FRAME_COMMIT_TIMEOUT_MS = PHASE_TIMEOUT_MS
        private const val CARET_PIXEL_MARGIN = 2
        private const val CARET_MATCH_FRACTION = .5f
        private const val ARGB_8888_BYTES_PER_PIXEL = Int.SIZE_BYTES
        private const val COLOR_TOLERANCE = 12
        private const val DECORATION_LINE = "underline"
        private const val DECORATION_STYLE = "double"
        private const val ATOM_TYPE = "horizontal_rule"
        private const val ATOM_RENDER_TYPE = "voidBlock"
        private const val ATOM_SELECTION_TYPE = "text"
        private const val ATOM_HTML_PREFIX = "<hr"
        private const val PIXEL_FAILURE_PREFIX = "caret-phase-pixel-failure"
        private const val ANIMATION_PHASE_FAILURE_PREFIX = "caret-animation-phase-failure"
        private const val ANIMATION_PHASE_ACTUAL = "actual"
        private const val ANIMATION_PHASE_INLINE = "inline"
        private const val ATOM_HEIGHT_DP = 48f
        private const val COMPOSITION_SESSION = "hardware-caret-composition"
        private const val COMPOSITION_CANCEL_CAUSE = "consumer"
        private const val COMPOSITION_TEXT = "A😀B"
        private const val NATIVE_EDIT = "caret-native-edit"
        private const val OWN_SCROLL_X = 1
        private const val OWN_SCROLL_Y = 2
        private const val ANCESTOR_SCROLL_Y = 4
        private const val SCROLL_PARAGRAPH_COUNT = 12
        private val SCROLL_PARAGRAPH = """
            {
                "type": "paragraph",
                "content": [
                    {
                        "type": "text",
                        "text": "Scroll down, select a word, and continue typing in the production editor."
                    }
                ]
            }
        """.trimIndent()
        private const val EMPTY_DOCUMENT = """{"type":"doc","content":[{"type":"paragraph"}]}"""
        private const val PLACEHOLDER_TEXT = "Type here"
        private val ATOM_DOCUMENT = """
            {
                "type": "doc",
                "content": [
                    {
                        "type": "paragraph",
                        "content": [{ "type": "text", "text": "Before atom" }]
                    },
                    { "type": "horizontal_rule" },
                    {
                        "type": "paragraph",
                        "content": [{ "type": "text", "text": "After atom" }]
                    }
                ]
            }
        """.trimIndent()
        private val GLYPH_COLOR = Color.rgb(19, 117, 231)
        private val CHANGED_GLYPH_COLOR = Color.rgb(231, 19, 117)
        private val VIEWPORT_SELECTION_COLOR = Color.rgb(11, 173, 61)
        private val DECORATION_COLOR = Color.rgb(231, 37, 19)

        private fun closeColor(actual: Int, expected: Int): Boolean =
            kotlin.math.abs(Color.red(actual) - Color.red(expected)) <= COLOR_TOLERANCE &&
                kotlin.math.abs(Color.green(actual) - Color.green(expected)) <= COLOR_TOLERANCE &&
                kotlin.math.abs(Color.blue(actual) - Color.blue(expected)) <= COLOR_TOLERANCE
    }
}
