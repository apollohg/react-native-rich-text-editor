package com.apollohg.editor

import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class EditorPasteModeExpoViewTest : NativeEditorExpoViewTestFixture() {
    @Test
    fun `paste mode updates live through the Expo view`() {
        val expoContext = testExpoContext(RuntimeEnvironment.getApplication())
        val view = NativeEditorExpoView(expoContext.context, expoContext.appContext)

        assertEquals(EditorPasteMode.RICH, view.richTextView.editorEditText.pasteMode)
        view.setPasteMode("plainText")
        assertEquals(EditorPasteMode.PLAIN_TEXT, view.richTextView.editorEditText.pasteMode)
        view.setPasteMode("disabled")
        assertEquals(EditorPasteMode.DISABLED, view.richTextView.editorEditText.pasteMode)
        view.setPasteMode("unexpected")
        assertEquals(EditorPasteMode.RICH, view.richTextView.editorEditText.pasteMode)
    }
}
