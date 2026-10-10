package com.apollohg.editor

import com.apollohg.editor.tables.TableLayoutDirection
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class EditorTableDirectionExpoViewTest : NativeEditorExpoViewTestFixture() {
    @Test
    fun `table direction prop reaches the editor and ignores unknown values`() {
        val expoContext = testExpoContext(RuntimeEnvironment.getApplication())
        val view = NativeEditorExpoView(expoContext.context, expoContext.appContext)

        assertEquals(null, view.richTextView.tableDirection)
        view.setTableDirection("rtl")
        assertEquals(TableLayoutDirection.RIGHT_TO_LEFT, view.richTextView.tableDirection)
        view.setTableDirection("ltr")
        assertEquals(TableLayoutDirection.LEFT_TO_RIGHT, view.richTextView.tableDirection)
        view.setTableDirection("auto")
        assertEquals(null, view.richTextView.tableDirection)
        view.setTableDirection("rtl")
        view.setTableDirection(null)
        assertEquals(null, view.richTextView.tableDirection)
    }
}
