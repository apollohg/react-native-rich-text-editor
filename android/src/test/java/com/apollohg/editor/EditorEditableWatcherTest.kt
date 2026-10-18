package com.apollohg.editor

import android.text.Annotation
import android.text.Editable
import android.text.InputFilter
import android.text.NoCopySpan
import android.text.SpanWatcher
import android.text.Spannable
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.SpannedString
import android.text.TextWatcher
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [24, 34])
class EditorEditableWatcherTest {
    @Test
    fun `rendered snapshot is shared until any span or text mutation`() {
        val text = EditorEditable("alpha beta")
        val style = android.text.style.StyleSpan(android.graphics.Typeface.BOLD)
        text.setSpan(style, 0, 5, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        val first = immutableRenderedText(text, excludeNoCopySpans = true)
        assertSame(
            "an unchanged editable reuses its snapshot",
            first,
            immutableRenderedText(text, excludeNoCopySpans = true)
        )

        fun assertFresh(step: String, mutate: () -> Unit) {
            val before = immutableRenderedText(text, excludeNoCopySpans = true)
            mutate()
            val after = immutableRenderedText(text, excludeNoCopySpans = true)
            assertFalse("$step must produce a new snapshot", before === after)
            assertEquals("$step text", text.toString(), after.toString())
            assertEquals(
                "$step spans",
                text.getSpans(0, text.length, Any::class.java)
                    .filterNot { it is NoCopySpan }
                    .map { listOf(it, text.getSpanStart(it), text.getSpanEnd(it)) },
                after.getSpans(0, after.length, Any::class.java)
                    .map { listOf(it, after.getSpanStart(it), after.getSpanEnd(it)) }
            )
        }
        assertFresh("moved span") { text.setSpan(style, 6, 10, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE) }
        assertFresh("removed span") { text.removeSpan(style) }
        assertFresh("inserted text") { text.insert(0, "x") }
        assertFresh("cleared spans") { text.clearSpans() }
    }

    @Test
    fun `rendered snapshot survives selection and other no copy span changes`() {
        val text = EditorEditable("alpha beta")
        text.setSpan(
            android.text.style.StyleSpan(android.graphics.Typeface.BOLD),
            0,
            5,
            Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
        )
        val snapshot = immutableRenderedText(text, excludeNoCopySpans = true)
        val watcher = object : SpanWatcher, NoCopySpan {
            override fun onSpanAdded(text: Spannable, what: Any, start: Int, end: Int) = Unit
            override fun onSpanRemoved(text: Spannable, what: Any, start: Int, end: Int) = Unit
            override fun onSpanChanged(
                text: Spannable,
                what: Any,
                oldStart: Int,
                oldEnd: Int,
                newStart: Int,
                newEnd: Int
            ) = Unit
        }

        android.text.Selection.setSelection(text, 2, 7)
        text.setSpan(watcher, 0, text.length, Spanned.SPAN_INCLUSIVE_INCLUSIVE)
        android.text.Selection.setSelection(text, 4)
        text.removeSpan(watcher)

        assertSame(
            "selection moves and native watchers must not copy the whole document again",
            snapshot,
            immutableRenderedText(text, excludeNoCopySpans = true)
        )
    }

    @Test
    fun `composition markers keep the snapshot while composing styles refresh it`() {
        val text = EditorEditable("alpha beta")
        val snapshot = immutableRenderedText(text, excludeNoCopySpans = true)

        android.view.inputmethod.BaseInputConnection.setComposingSpans(text)

        assertSame(snapshot, immutableRenderedText(text, excludeNoCopySpans = true))
        val composingStyle = android.text.style.UnderlineSpan()
        text.setSpan(
            composingStyle,
            6,
            10,
            Spanned.SPAN_EXCLUSIVE_EXCLUSIVE or Spanned.SPAN_COMPOSING
        )
        val styled = immutableRenderedText(text, excludeNoCopySpans = true)
        assertFalse("a composing style must produce a new snapshot", snapshot === styled)
        assertEquals(
            listOf<Any>(composingStyle),
            styled.getSpans(0, styled.length, Any::class.java).toList()
        )
        android.view.inputmethod.BaseInputConnection.removeComposingSpans(text)
        assertFalse(
            "removing the composing style must produce a new snapshot",
            styled === immutableRenderedText(text, excludeNoCopySpans = true)
        )
    }

    @Test
    fun `watcher range queries and notifications match Android through replacement and removal`() {
        val fixture = Fixture()
        fixture.change { text, events ->
            text.setSpan(
                RecordingWatcher("primary", events),
                1,
                5,
                Spanned.SPAN_INCLUSIVE_INCLUSIVE
            )
            text.setSpan(Annotation("mark", "bold"), 2, 5, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        }
        fixture.change { text, _ ->
            val replacement = SpannableStringBuilder("日本😀").apply {
                setSpan(
                    Annotation("replacement", "bold"),
                    0,
                    length,
                    Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                )
                setSpan(NoCopySpan.Concrete(), 0, length, Spanned.SPAN_INCLUSIVE_INCLUSIVE)
            }
            text.replace(2, 3, SpannedString(replacement))
        }
        fixture.change { text, _ ->
            val watcher = text.getSpans(0, text.length, RecordingWatcher::class.java).single()
            text.setSpan(watcher, text.length, text.length, Spanned.SPAN_MARK_MARK)
            text.setSpan(
                Annotation("point", "end"),
                text.length,
                text.length,
                Spanned.SPAN_MARK_MARK
            )
        }
        fixture.change { text, _ -> text.delete(0, text.length) }
        fixture.change { text, _ ->
            text.clearSpans()
            text.append("after😀")
        }
        fixture.change { text, events ->
            text.setSpan(
                RecordingWatcher("restored", events),
                0,
                text.length,
                Spanned.SPAN_INCLUSIVE_INCLUSIVE
            )
            text.insert(1, "!")
        }
    }

    @Test
    fun `copied and filtered watchers retain Android priority and callback semantics`() {
        val fixture = Fixture()
        fixture.change { text, events ->
            text.setSpan(
                RecordingWatcher("original", events),
                0,
                text.length,
                Spanned.SPAN_INCLUSIVE_INCLUSIVE
            )
        }
        fixture.change { text, events ->
            val replacement = SpannableStringBuilder("copied😀")
            replacement.setSpan(
                RecordingWatcher("copied", events),
                0,
                replacement.length,
                PRIORITY_FLAGS
            )
            text.replace(2, 5, replacement)
        }
        fixture.change { text, events ->
            text.filters = arrayOf(
                InputFilter { _, _, _, _, _, _ ->
                    SpannableStringBuilder("filtered😀").apply {
                        setSpan(RecordingWatcher("filtered", events), 0, length, PRIORITY_FLAGS)
                    }
                }
            )
            text.replace(1, 2, "input")
            text.filters = emptyArray()
        }
        fixture.change { text, _ ->
            text.getSpans(0, text.length, RecordingWatcher::class.java)
                .filter { it.label != "filtered" }.forEach(text::removeSpan)
            text.setSpan(Annotation("mark", "italic"), 1, 3, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
            text.insert(0, "prefix")
        }
    }

    @Test
    fun `watchers introduced into mutable replacement during callbacks remain visible`() {
        val fixture = Fixture()
        fixture.change { text, events ->
            val replacement = SpannableStringBuilder("source😀")
            var introduced = false
            text.setSpan(
                RecordingWatcher("injector", events, beforeChange = {
                    if (!introduced) {
                        introduced = true
                        replacement.setSpan(
                            RecordingWatcher("late", events),
                            0,
                            replacement.length,
                            Spanned.SPAN_INCLUSIVE_INCLUSIVE
                        )
                    }
                }),
                0,
                text.length,
                Spanned.SPAN_INCLUSIVE_INCLUSIVE
            )
            text.replace(2, 5, replacement)
        }
        fixture.change { text, _ ->
            text.getSpans(0, text.length, RecordingWatcher::class.java)
                .filter { it.label == "injector" }.forEach(text::removeSpan)
            text.insert(1, "!")
        }
    }

    @Test
    fun `reentrant watcher replacement preserves recipients and subsequent text changes`() {
        val fixture = Fixture()
        fixture.change { text, events ->
            text.setSpan(
                RecordingWatcher("reentrant", events, replaceOnTrigger = true),
                0,
                text.length,
                Spanned.SPAN_INCLUSIVE_INCLUSIVE
            )
        }
        fixture.change { text, _ ->
            text.setSpan(Annotation("trigger", "replace"), 1, 3, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        }
        fixture.change { text, _ ->
            text.replace(1, 5, "長😀")
            text.clearSpans()
        }
    }

    @Test
    fun `renderer replacement freezes ranges and retains span identities and copied watchers`() {
        val fixture = Fixture()
        fixture.change { text, events ->
            val source = SpannableStringBuilder(SNAPSHOT_TEXT)
            val metadata = RenderMetadataAnnotation(
                RenderBridge.NATIVE_DOC_POS_ANNOTATION,
                SNAPSHOT_DOC_POS
            )
            val marker = NoCopySpan.Concrete()
            val watcher = RecordingWatcher("snapshot", events)
            source.setSpan(metadata, 0, source.length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
            source.setSpan(marker, 0, source.length, Spanned.SPAN_INCLUSIVE_INCLUSIVE)
            source.setSpan(watcher, 0, source.length, PRIORITY_FLAGS)
            val replacement = immutableRenderedText(source)
            source.clearSpans()
            source.replace(0, source.length, INITIAL_TEXT)
            metadata.updateValue(UPDATED_DOC_POS)

            assertEquals("replacement characters are frozen", SNAPSHOT_TEXT, replacement.toString())
            for (span in listOf(metadata, marker, watcher)) {
                assertSame(
                    "replacement keeps the original ${span.javaClass.simpleName}",
                    span,
                    replacement.getSpans(0, replacement.length, Any::class.java)
                        .single { it.javaClass == span.javaClass }
                )
                assertEquals("frozen span start", 0, replacement.getSpanStart(span))
                assertEquals("frozen span end", SNAPSHOT_TEXT.length, replacement.getSpanEnd(span))
            }
            assertEquals(
                "renderer metadata stays shared",
                UPDATED_DOC_POS,
                replacement.getSpans(0, replacement.length, RenderMetadataAnnotation::class.java)
                    .single().value
            )
            text.replace(0, text.length, replacement)
            for (span in listOf(metadata, marker, watcher)) {
                assertSame(
                    "Android replacement retains ${span.javaClass.simpleName}",
                    span,
                    text.getSpans(0, text.length, Any::class.java)
                        .single { it.javaClass == span.javaClass }
                )
            }
        }
        fixture.change { text, _ -> text.insert(0, "!") }
    }

    private class Fixture {
        private val actual = EditorTextSurface(RuntimeEnvironment.getApplication()).apply {
            setText(INITIAL_TEXT)
        }.editableText.apply { clearSpans() }
        private val expected = SpannableStringBuilder(INITIAL_TEXT)
        private val actualEvents = mutableListOf<String>()
        private val expectedEvents = mutableListOf<String>()

        fun change(mutation: (Editable, MutableList<String>) -> Unit) {
            mutation(actual, actualEvents)
            mutation(expected, expectedEvents)
            assertEquals("complete watcher callback sequence", expectedEvents, actualEvents)
            assertEquals("authoritative buffer text", expected.toString(), actual.toString())
            fun spans(text: Spanned) =
                text.getSpans(0, (text as CharSequence).length, Any::class.java)
                    .map {
                        listOf(
                            spanName(it),
                            text.getSpanStart(it),
                            text.getSpanEnd(it),
                            text.getSpanFlags(it)
                        )
                    }
            assertEquals(
                "span identity names, order, bounds and flags",
                spans(expected),
                spans(actual)
            )
            val offsets = listOf(-1, 0, 1, 2, actual.length - 1, actual.length, actual.length + 1)
            for (start in offsets) {
                for (end in offsets) {
                    for (kind in listOf(SpanWatcher::class.java, TextWatcher::class.java)) {
                        assertEquals(
                            "${kind.simpleName} query $start..$end",
                            expected.getSpans(start, end, kind).map(::spanName),
                            actual.getSpans(start, end, kind).map(::spanName)
                        )
                    }
                }
            }
        }
    }

    private class RecordingWatcher(
        val label: String,
        private val events: MutableList<String>,
        private val replaceOnTrigger: Boolean = false,
        private val beforeChange: (() -> Unit)? = null
    ) : SpanWatcher,
        TextWatcher {
        override fun beforeTextChanged(text: CharSequence?, start: Int, count: Int, after: Int) {
            events += "$label:before:$start:$count:$after:$text"
            beforeChange?.invoke()
        }
        override fun onTextChanged(text: CharSequence?, start: Int, before: Int, count: Int) {
            events += "$label:text:$start:$before:$count:$text"
        }
        override fun afterTextChanged(text: Editable?) {
            events += "$label:after:$text"
        }
        override fun onSpanAdded(text: Spannable?, what: Any?, start: Int, end: Int) {
            events += "$label:add:${spanName(what)}:$start:$end"
            if (replaceOnTrigger && what is Annotation && what.key == "trigger") {
                requireNotNull(text).setSpan(
                    RecordingWatcher("replacement", events),
                    0,
                    text.length,
                    PRIORITY_FLAGS
                )
                text.removeSpan(this)
            }
        }
        override fun onSpanRemoved(text: Spannable?, what: Any?, start: Int, end: Int) {
            events += "$label:remove:${spanName(what)}:$start:$end"
        }
        override fun onSpanChanged(
            text: Spannable?,
            what: Any?,
            oldStart: Int,
            oldEnd: Int,
            newStart: Int,
            newEnd: Int
        ) {
            events += "$label:move:${spanName(what)}:$oldStart:$oldEnd:$newStart:$newEnd"
        }
    }

    private companion object {
        const val INITIAL_TEXT = "abc😀def\nxy"
        const val SNAPSHOT_TEXT = "source😀"
        const val SNAPSHOT_DOC_POS = "12"
        const val UPDATED_DOC_POS = "13"
        const val PRIORITY_FLAGS =
            Spanned.SPAN_INCLUSIVE_INCLUSIVE or (1 shl Spanned.SPAN_PRIORITY_SHIFT)

        fun spanName(span: Any?): String = when (span) {
            is RecordingWatcher -> span.label
            is Annotation -> "${span.key}:${span.value}"
            else -> span?.javaClass?.simpleName.orEmpty()
        }
    }
}
