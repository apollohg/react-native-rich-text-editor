package com.apollohg.editor

import android.content.Context
import android.os.Parcel
import android.text.Annotation
import android.text.Spanned
import android.text.TextUtils
import android.view.KeyEvent
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.CorrectionInfo
import android.view.inputmethod.EditorInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [24, 34])
class EditorSurfaceInputConnectionTest {
    private class Surface(context: Context) : EditorTextSurface(context) {
        var published = 0
        val keys = mutableListOf<Int>()
        override fun onSurfaceInputStateChanged() {
            published++
        }
        override fun dispatchKeyEvent(event: KeyEvent): Boolean {
            keys += event.keyCode
            return true
        }
    }

    @Test
    fun `renderer metadata keeps its current value through direct and text parceling`() {
        val metadata = RenderMetadataAnnotation(RenderBridge.NATIVE_DOC_POS_ANNOTATION, "1")
        metadata.updateValue("2")
        val directParcel = Parcel.obtain()
        val textParcel = Parcel.obtain()
        try {
            metadata.writeToParcel(directParcel, 0)
            directParcel.setDataPosition(0)
            assertEquals("2", Annotation(directParcel).value)
            val text = android.text.SpannableStringBuilder("a😀b")
            text.setSpan(metadata, 1, 3, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
            TextUtils.writeToParcel(text, textParcel, 0)
            textParcel.setDataPosition(0)
            val restored = TextUtils.CHAR_SEQUENCE_CREATOR.createFromParcel(textParcel) as Spanned
            val annotation = restored.getSpans(1, 3, Annotation::class.java).single()
            assertEquals(RenderBridge.NATIVE_DOC_POS_ANNOTATION, annotation.key)
            assertEquals("2", annotation.value)
            assertEquals(1, restored.getSpanStart(annotation))
            assertEquals(3, restored.getSpanEnd(annotation))
        } finally {
            directParcel.recycle()
            textParcel.recycle()
        }
    }

    @Test
    fun `annotation batch publishes once and restores the text change watcher`() {
        val surface = Surface(RuntimeEnvironment.getApplication())
        surface.setText("a😀b")
        val editable = surface.editableText
        val input = requireNotNull(surface.onCreateInputConnection(EditorInfo()))
        val position = Annotation(RenderBridge.NATIVE_DOC_POS_ANNOTATION, "1")
        editable.setSpan(position, 1, 3, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        surface.published = 0
        input.beginBatchEdit()
        surface.updateAnnotationSpans {
            editable.removeSpan(position)
            editable.setSpan(
                Annotation(RenderBridge.NATIVE_DOC_POS_ANNOTATION, "2"),
                1,
                3,
                Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
            )
        }
        assertSame(editable, surface.editableText)
        assertEquals(0, surface.published)
        input.endBatchEdit()
        assertEquals(1, surface.published)
        assertEquals("2", editable.getSpans(1, 3, Annotation::class.java).single().value)
        surface.published = 0
        surface.setSelection(editable.length)
        assertTrue(input.commitText("!", 1))
        assertEquals("a😀b!", surface.text.toString())
        assertTrue("text mutation must still publish input state", surface.published > 0)
    }

    @Test
    fun `base composition uses the surface buffer and defers reporting until batch end`() {
        val surface = Surface(RuntimeEnvironment.getApplication())
        surface.setText("a😀b")
        surface.setSelection(1, 3)
        val editable = surface.editableText
        val input = requireNotNull(surface.onCreateInputConnection(EditorInfo()))
        surface.published = 0
        input.beginBatchEdit()
        input.setComposingText("日本", 1)
        assertSame(editable, surface.editableText)
        assertEquals("a日本b", editable.toString())
        assertEquals(1, BaseInputConnection.getComposingSpanStart(editable))
        assertEquals(3, BaseInputConnection.getComposingSpanEnd(editable))
        assertEquals(0, surface.published)
        input.endBatchEdit()
        assertEquals(1, surface.published)
    }

    @Test
    fun `base correction notification does not replace text or remove composition`() {
        val surface = Surface(RuntimeEnvironment.getApplication())
        surface.setText("before")
        surface.setSelection(0, 6)
        val input = requireNotNull(surface.onCreateInputConnection(EditorInfo()))
        input.setComposingText("after", 1)
        assertTrue(input.commitCorrection(CorrectionInfo(0, "before", "after")))
        assertEquals("after", surface.text.toString())
        assertEquals(0, BaseInputConnection.getComposingSpanStart(surface.editableText))
        assertEquals(5, BaseInputConnection.getComposingSpanEnd(surface.editableText))
    }

    @Test
    fun `replacement base connection retires raw mutation and key delivery`() {
        val surface = Surface(RuntimeEnvironment.getApplication())
        surface.setText("safe")
        val retired = requireNotNull(surface.onCreateInputConnection(EditorInfo()))
        surface.onCreateInputConnection(EditorInfo())
        retired.commitText("bad", 1)
        retired.setComposingText("bad", 1)
        retired.sendKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_A))
        assertEquals("safe", surface.text.toString())
        assertTrue(surface.keys.isEmpty())
    }
}
